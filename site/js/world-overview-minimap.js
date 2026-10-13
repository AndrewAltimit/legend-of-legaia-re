/* world-overview-minimap.js - the world-overview page's top-down map.
 *
 * A 2D plan of the kingdom the 3D scene beside it is showing, drawn from the
 * same disc data in the same world frame: the walk heightfield's land cells as
 * the silhouette, the walk `.MAP` stamps as dots (placed objects and the
 * decoration layer as two families), and the kingdom's records of the game's
 * own world-map label table as named markers.
 *
 * Orientation matches the 3D scene's default camera (`buildWorldOrbitVp` at
 * yaw 0): screen-right = world +X, screen-up = world +Z. The plot is fitted to
 * the land's bounding box with a margin and letterboxed, never stretched, and
 * nothing is clamped to the border: a record with no position is left out and
 * counted in the legend by the page.
 *
 * Everything drawn comes from the reader's disc at runtime; without one the
 * canvas says so. Colours are CSS custom properties on the side column, so the
 * legend swatches and the dots cannot drift apart and both themes are covered
 * by the stylesheet.
 *
 * Classic script; loads before world-overview-app.js. Published as
 * window.LegaiaWorldOverviewMinimap. */

(function () {
  const GRID = 128;          /* walk grid cells per axis */
  const CELL = 128;          /* world units per cell */
  const MARGIN = 14;         /* CSS px kept clear around the fitted land */
  const HIT_RADIUS = 9;      /* CSS px a pointer may miss a marker by */
  const FONT = '600 10.5px system-ui, -apple-system, "Segoe UI", sans-serif';

  /* The PSX texture pages the terrain atlas keeps its water tiles on
   * (docs/subsystems/world-map.md, "Ground texturing"). */
  const WATER_TPAGES = new Set([0x1B, 0x1C]);

  function cssColor(el, name, fallback) {
    const v = getComputedStyle(el).getPropertyValue(name).trim();
    return v || fallback;
  }

  function parseRgb(css) {
    const probe = document.createElement('canvas').getContext('2d');
    probe.fillStyle = '#000';
    probe.fillStyle = css;
    const hex = probe.fillStyle;
    if (hex.charAt(0) === '#') {
      return [parseInt(hex.slice(1, 3), 16), parseInt(hex.slice(3, 5), 16), parseInt(hex.slice(5, 7), 16)];
    }
    const m = hex.match(/[\d.]+/g) || [0, 0, 0];
    return [+m[0], +m[1], +m[2]];
  }

  /* Reduce the heightfield to a 128x128 land mask: 1 = a land cell, 0 = sea,
   * river or lake (the heightfield covers the whole grid, water included, so
   * the texture page is what tells the two apart). `positions` is the
   * flattened per-vertex [x, y, z], four vertices per cell quad; `cbaTsb` the
   * index-aligned [clut, tpage] pairs. Returns { cells, bounds } with the
   * land's bounding box in world units, or null when there is no land. */
  function classifyGround(positions, cbaTsb) {
    if (!positions || positions.length < 12) return null;
    const cells = new Uint8Array(GRID * GRID);
    let c0 = GRID, c1 = -1, r0 = GRID, r1 = -1;
    const quads = Math.floor(positions.length / 12);
    for (let q = 0; q < quads; q++) {
      let minX = Infinity, minZ = Infinity;
      for (let v = 0; v < 4; v++) {
        const x = positions[(q * 4 + v) * 3];
        const z = positions[(q * 4 + v) * 3 + 2];
        if (x < minX) minX = x;
        if (z < minZ) minZ = z;
      }
      const col = Math.round(minX / CELL);
      const row = Math.round(minZ / CELL);
      if (col < 0 || col >= GRID || row < 0 || row >= GRID) continue;
      const tpage = cbaTsb ? (cbaTsb[q * 4 * 2 + 1] & 0x1F) : 0;
      if (WATER_TPAGES.has(tpage)) continue;
      cells[row * GRID + col] = 1;
      if (col < c0) c0 = col;
      if (col > c1) c1 = col;
      if (row < r0) r0 = row;
      if (row > r1) r1 = row;
    }
    if (c1 < 0) return null;
    return {
      cells,
      bounds: { x0: c0 * CELL, x1: (c1 + 1) * CELL, z0: r0 * CELL, z1: (r1 + 1) * CELL },
    };
  }

  function rectsOverlap(a, b) {
    return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
  }

  function createMinimap(opts) {
    const canvas = opts.canvas;
    const $hover = opts.hover;
    const ctx = canvas.getContext('2d');
    const themed = canvas.parentElement || canvas;
    let state = null;      /* { title, data } - data null = nothing to plot */
    let hits = [];         /* [{ x, y, kind, item, pri }] in CSS px */
    let highlight = null;  /* the hit entry under the pointer / list row */
    let landCache = null;  /* { key, bitmap } */

    function cssSize() {
      const r = canvas.getBoundingClientRect();
      const w = Math.max(1, Math.round(r.width || canvas.clientWidth || 280));
      const h = Math.max(1, Math.round(r.height || canvas.clientHeight || w));
      return [w, h];
    }

    /* Size the backing store by devicePixelRatio and return the CSS-pixel
     * drawing size; every draw call below is in CSS pixels. */
    function prepare() {
      const [w, h] = cssSize();
      const dpr = Math.max(1, Math.min(4, window.devicePixelRatio || 1));
      const bw = Math.round(w * dpr), bh = Math.round(h * dpr);
      if (canvas.width !== bw || canvas.height !== bh) {
        canvas.width = bw;
        canvas.height = bh;
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      return [w, h];
    }

    function palette() {
      return {
        sea:    cssColor(themed, '--wo-map-sea', '#101a2b'),
        land:   cssColor(themed, '--wo-map-land', '#33402f'),
        place:  cssColor(themed, '--wo-map-place', '#F4B41A'),
        object: cssColor(themed, '--wo-map-object', '#56B6F5'),
        deco:   cssColor(themed, '--wo-map-deco', '#7aa568'),
        ink:    cssColor(themed, '--wo-map-ink', '#eef0f4'),
        halo:   cssColor(themed, '--wo-map-halo', 'rgba(8,10,18,0.85)'),
      };
    }

    /* The land raster: one pixel per walk cell, world +Z up. Rebuilt only
     * when the kingdom or the theme colours change. */
    function landBitmap(ground, pal) {
      const key = state.title + '|' + pal.land;
      if (landCache && landCache.key === key && landCache.cells === ground.cells) return landCache.bitmap;
      const bmp = document.createElement('canvas');
      bmp.width = GRID;
      bmp.height = GRID;
      const bctx = bmp.getContext('2d');
      const img = bctx.createImageData(GRID, GRID);
      const rgb = parseRgb(pal.land);
      for (let row = 0; row < GRID; row++) {
        for (let col = 0; col < GRID; col++) {
          if (!ground.cells[row * GRID + col]) continue;
          const o = ((GRID - 1 - row) * GRID + col) * 4;
          img.data[o] = rgb[0];
          img.data[o + 1] = rgb[1];
          img.data[o + 2] = rgb[2];
          img.data[o + 3] = 255;
        }
      }
      bctx.putImageData(img, 0, 0);
      landCache = { key, cells: ground.cells, bitmap: bmp };
      return bmp;
    }

    function message(w, h, pal, lines) {
      ctx.fillStyle = pal.ink;
      ctx.font = '12px system-ui, -apple-system, "Segoe UI", sans-serif';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      const y0 = h / 2 - (lines.length - 1) * 9;
      lines.forEach((line, i) => ctx.fillText(line, w / 2, y0 + i * 18));
      ctx.textAlign = 'left';
    }

    function draw() {
      const [w, h] = prepare();
      const pal = palette();
      hits = [];
      ctx.fillStyle = pal.sea;
      ctx.fillRect(0, 0, w, h);
      const report = { kingdom: state ? state.key : null, plotted: false };
      window.__woMinimap = report;
      if (!state) return;
      const data = state.data;
      if (!data || !data.ground) {
        message(w, h, pal, state.hint || ['Load a disc image to plot', state.title]);
        return;
      }

      /* Fit the land's bounding box, widened to hold every marker (a bridge
       * or a label may sit over water), letterboxed: one scale for both
       * axes, so nothing is stretched and nothing lands outside the margin. */
      const b = Object.assign({}, data.ground.bounds);
      for (const m of data.stamps.concat(data.labels)) {
        if (m.x < b.x0) b.x0 = m.x;
        if (m.x > b.x1) b.x1 = m.x;
        if (m.z < b.z0) b.z0 = m.z;
        if (m.z > b.z1) b.z1 = m.z;
      }
      const spanX = b.x1 - b.x0, spanZ = b.z1 - b.z0;
      const scale = Math.min((w - 2 * MARGIN) / spanX, (h - 2 * MARGIN) / spanZ);
      const offX = (w - spanX * scale) / 2;
      const offY = (h - spanZ * scale) / 2;
      const sx = (x) => offX + (x - b.x0) * scale;
      const sy = (z) => offY + (b.z1 - z) * scale;   /* world +Z is up */

      ctx.imageSmoothingEnabled = false;
      ctx.drawImage(landBitmap(data.ground, pal),
        sx(0), sy(GRID * CELL), GRID * CELL * scale, GRID * CELL * scale);

      /* Decoration layer first (it is the densest), then placed objects,
       * then the named places on top. */
      ctx.fillStyle = pal.deco;
      for (const s of data.stamps) {
        if (s.placed) continue;
        const x = sx(s.x), y = sy(s.z);
        ctx.fillRect(x - 0.9, y - 0.9, 1.8, 1.8);
        hits.push({ x, y, kind: 'deco', item: s, pri: 2 });
      }
      ctx.fillStyle = pal.object;
      ctx.strokeStyle = pal.halo;
      ctx.lineWidth = 1;
      for (const s of data.stamps) {
        if (!s.placed) continue;
        const x = sx(s.x), y = sy(s.z);
        ctx.fillRect(x - 2.5, y - 2.5, 5, 5);
        ctx.strokeRect(x - 2.5, y - 2.5, 5, 5);
        hits.push({ x, y, kind: 'object', item: s, pri: 1 });
      }
      const placeHits = [];
      for (const l of data.labels) {
        const x = sx(l.x), y = sy(l.z);
        ctx.beginPath();
        ctx.arc(x, y, 4, 0, 2 * Math.PI);
        ctx.fillStyle = pal.place;
        ctx.fill();
        ctx.lineWidth = 1.5;
        ctx.strokeStyle = pal.halo;
        ctx.stroke();
        const hit = { x, y, kind: 'place', item: l, pri: 0 };
        hits.push(hit);
        placeHits.push(hit);
      }

      /* Labels: named places only, placed greedily so no two overlap and
       * none covers another place's marker. One that fits nowhere is left
       * to the hover readout and the list beside the map. */
      ctx.font = FONT;
      ctx.textBaseline = 'middle';
      ctx.lineJoin = 'round';
      const taken = placeHits.map((p) => ({ x: p.x - 5, y: p.y - 5, w: 10, h: 10, owner: p }));
      let drawn = 0;
      for (const p of placeHits) {
        const tw = Math.ceil(ctx.measureText(p.item.name).width);
        const th = 12;
        const cands = [
          { x: p.x + 7, y: p.y - th / 2 },
          { x: p.x - 7 - tw, y: p.y - th / 2 },
          { x: p.x - tw / 2, y: p.y - 7 - th },
          { x: p.x - tw / 2, y: p.y + 7 },
          { x: p.x + 6, y: p.y - 5 - th },
          { x: p.x + 6, y: p.y + 5 },
          { x: p.x - 6 - tw, y: p.y - 5 - th },
          { x: p.x - 6 - tw, y: p.y + 5 },
        ];
        const spot = cands.map((c) => ({ x: c.x, y: c.y, w: tw, h: th })).find((r) =>
          r.x >= 2 && r.y >= 2 && r.x + r.w <= w - 2 && r.y + r.h <= h - 2 &&
          !taken.some((t) => t.owner !== p && rectsOverlap(r, t)));
        if (!spot) continue;
        taken.push(spot);
        ctx.lineWidth = 3;
        ctx.strokeStyle = pal.halo;
        ctx.strokeText(p.item.name, spot.x, spot.y + th / 2);
        ctx.fillStyle = pal.ink;
        ctx.fillText(p.item.name, spot.x, spot.y + th / 2);
        drawn++;
      }

      if (highlight) {
        const live = hits.find((d) => d.kind === highlight.kind && d.item === highlight.item);
        if (live) {
          ctx.beginPath();
          ctx.arc(live.x, live.y, 7.5, 0, 2 * Math.PI);
          ctx.lineWidth = 2;
          ctx.strokeStyle = pal.ink;
          ctx.stroke();
        }
      }

      /* Headless-verification hook (same spirit as `__woCam`). Not used by
       * page code. */
      const inside = (d) => d.x >= 0 && d.x <= w && d.y >= 0 && d.y <= h;
      const onEdge = (d) => d.x < MARGIN - 1 || d.x > w - MARGIN + 1 || d.y < MARGIN - 1 || d.y > h - MARGIN + 1;
      report.plotted = true;
      report.places = placeHits.length;
      report.objects = data.stamps.filter((s) => s.placed).length;
      report.decorations = data.stamps.length - report.objects;
      report.labelsDrawn = drawn;
      state.labelsDrawn = drawn;
      report.offCanvas = hits.filter((d) => !inside(d)).length;
      report.inMargin = hits.filter(onEdge).length;
      report.bounds = b;
      report.scale = scale;
      report.css = [w, h];
      report.backing = [canvas.width, canvas.height];
    }

    function describe(hit) {
      const it = hit.item;
      if (hit.kind === 'place') {
        return `${it.name}  -  world (${it.x}, ${it.z})`;
      }
      const what = hit.kind === 'object' ? 'placed object' : 'decoration';
      return `${what}  -  pack mesh ${it.slot}  world (${it.x}, ${it.z})`;
    }

    function setHover(hit) {
      if ($hover) {
        if (hit) $hover.textContent = describe(hit);
        else $hover.innerHTML = '&nbsp;';
      }
      const same = (hit && highlight && hit.kind === highlight.kind && hit.item === highlight.item) ||
        (!hit && !highlight);
      if (same) return;
      highlight = hit;
      draw();
    }

    /* Nearest marker to a pointer, the more specific family winning a tie
     * within reach: a named place over an object over a decoration. */
    function pick(ev) {
      const rect = canvas.getBoundingClientRect();
      const mx = ev.clientX - rect.left, my = ev.clientY - rect.top;
      let best = null, bestScore = Infinity;
      for (const d of hits) {
        const dx = d.x - mx, dy = d.y - my;
        const r2 = dx * dx + dy * dy;
        if (r2 > HIT_RADIUS * HIT_RADIUS) continue;
        const score = d.pri * 1000 + r2;
        if (score < bestScore) { bestScore = score; best = d; }
      }
      return best;
    }

    canvas.addEventListener('pointermove', (ev) => setHover(pick(ev)));
    canvas.addEventListener('pointerdown', (ev) => setHover(pick(ev)));
    canvas.addEventListener('pointerleave', (ev) => {
      /* A touch keeps its readout after the finger lifts. */
      if (ev.pointerType === 'mouse') setHover(null);
    });

    /* Redraw when the theme flips or the column resizes the canvas. */
    new MutationObserver(() => { landCache = null; draw(); })
      .observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    if (window.matchMedia) {
      const mq = window.matchMedia('(prefers-color-scheme: dark)');
      const onScheme = () => { landCache = null; draw(); };
      if (mq.addEventListener) mq.addEventListener('change', onScheme);
    }
    if (window.ResizeObserver) new ResizeObserver(() => draw()).observe(canvas);

    return {
      /* `data` = { ground: classifyGround(...), stamps: [{x, z, slot,
       * placed}], labels: [{x, z, name}] }, or null to show `hint` (an array
       * of lines) in place of a plot. */
      show(key, title, data, hint) {
        state = { key, title, data, hint, labelsDrawn: 0 };
        highlight = null;
        if ($hover) $hover.innerHTML = '&nbsp;';
        draw();
        return { labelsDrawn: state.labelsDrawn };
      },
      /* Ring the marker of one named place (the list beside the map). */
      highlightPlace(label) {
        const hit = label ? hits.find((d) => d.kind === 'place' && d.item === label) : null;
        setHover(hit || null);
      },
    };
  }

  window.LegaiaWorldOverviewMinimap = { createMinimap, classifyGround };
})();
