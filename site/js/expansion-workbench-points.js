/*
 * Point-list editors for the modes tab: a race's centre line (`points`, world
 * units in the race's own space, or `waypoints`, map tiles) and a prop-hunt
 * arena's `blockers` (rectangles in its arena units). Each is a row list -
 * typed fields, add, duplicate, reorder, remove - beside a small 2D pad: a
 * click on empty pad adds a point (a drag draws a blocker), a drag on a point
 * or a blocker moves it, and the row under edit is outlined.
 *
 * Every row carries its validator path (`races[0].points[3]`), and every edit
 * goes through `ctx.changed`, so it is one step of the page's undo.
 */
import { h } from './expansion-workbench-forms.js';

const intOr = (v, d) => { const n = parseInt(v, 10); return Number.isFinite(n) ? n : d; };

/**
 * `o`: { title, hint, kind: 'point' | 'rect', tiles: bool, bounds: () => [x0, y0, x1, y1],
 *        names: [..], make: () => value, background: (g, view) => void, decor: (g, view) => void }
 */
export function pointListEd(ctx, parent, owner, key, path, o) {
  if (!Array.isArray(owner[key])) owner[key] = [];
  const arr = owner[key];
  const p = path ? `${path}.${key}` : key;
  const n = o.kind === 'rect' ? 4 : 2;
  const names = o.names || (o.kind === 'rect' ? ['x0', 'y0', 'x1', 'y1'] : ['x', 'z']);
  const wrap = h('div', { class: 'xw-field is-wide xw-points', 'data-path': p });
  wrap.append(h('span', {}, o.title, o.hint ? h('span', { class: 'xw-hint' }, ` ${o.hint}`) : null));
  const body = h('div', { class: 'xw-points-body' });
  const list = h('div', { class: 'xw-list-ed' });
  const cv = h('canvas', { class: 'xw-pad', width: 320, height: 240, 'aria-label': `${o.title}: click to add, drag to move` });
  body.append(cv, list);
  wrap.append(body);
  let focus = -1;

  // --- View: the bounds fitted into the pad, a margin around them.
  const view = () => {
    const pts = arr.flatMap((q) => (n === 4 ? [[q[0], q[1]], [q[2], q[3]]] : [q]));
    const b = o.bounds ? o.bounds() : null;
    let [x0, y0, x1, y1] = b || [0, 0, 0, 0];
    if (!b) for (const [x, y] of pts) { x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); }
    if (x1 - x0 < 1) x1 = x0 + (o.tiles ? 8 : 1000);
    if (y1 - y0 < 1) y1 = y0 + (o.tiles ? 6 : 1000);
    // Tiles span a whole tile each: the last one ends a tile past its index.
    const ex = o.tiles ? 1 : 0;
    const W = cv.width, H = cv.height, pad = o.tiles ? 4 : 14;
    const s = Math.min((W - 2 * pad) / (x1 - x0 + ex), (H - 2 * pad) / (y1 - y0 + ex));
    const ox = pad + ((W - 2 * pad) - s * (x1 - x0 + ex)) / 2, oy = pad + ((H - 2 * pad) - s * (y1 - y0 + ex)) / 2;
    const half = o.tiles ? 0.5 : 0;
    return {
      s, x0, y0, x1, y1,
      toPx: (x, y) => [ox + (x + half - x0) * s, oy + (y + half - y0) * s],
      fromPx: (px, py) => [(px - ox) / s + x0 - half, (py - oy) / s + y0 - half],
    };
  };

  const draw = () => {
    const g = cv.getContext('2d');
    const v = view();
    g.clearRect(0, 0, cv.width, cv.height);
    g.fillStyle = '#1b1d22';
    g.fillRect(0, 0, cv.width, cv.height);
    if (o.background) o.background(g, v);
    else {
      const [a, b] = v.toPx(v.x0, v.y0), [c, d] = v.toPx(v.x1, v.y1);
      g.strokeStyle = 'rgba(200,210,225,0.35)';
      g.setLineDash([4, 3]);
      g.strokeRect(a + 0.5, b + 0.5, c - a, d - b);
      g.setLineDash([]);
    }
    if (o.decor) o.decor(g, v);
    if (n === 4) {
      arr.forEach((q, i) => {
        const [a, b] = v.toPx(Math.min(q[0], q[2]), Math.min(q[1], q[3])), [c, d] = v.toPx(Math.max(q[0], q[2]), Math.max(q[1], q[3]));
        g.fillStyle = i === focus ? 'rgba(230,162,60,0.45)' : 'rgba(230,162,60,0.25)';
        g.fillRect(a, b, c - a, d - b);
        g.strokeStyle = i === focus ? '#ffffff' : '#e6a23c';
        g.lineWidth = 2;
        g.strokeRect(a + 1, b + 1, c - a - 2, d - b - 2);
      });
    } else if (arr.length) {
      g.strokeStyle = '#4aa3e8';
      g.lineWidth = 2;
      g.beginPath();
      arr.forEach((q, i) => { const [x, y] = v.toPx(q[0], q[1]); if (i) g.lineTo(x, y); else g.moveTo(x, y); });
      if (arr.length > 2) g.closePath();
      g.stroke();
      arr.forEach((q, i) => {
        const [x, y] = v.toPx(q[0], q[1]);
        g.fillStyle = i === 0 ? '#3ecf8e' : '#4aa3e8';
        g.strokeStyle = i === focus ? '#ffffff' : '#0b1a2a';
        g.beginPath();
        g.arc(x, y, 6, 0, Math.PI * 2);
        g.fill();
        g.stroke();
        g.fillStyle = '#06111d';
        g.font = 'bold 9px monospace';
        g.textAlign = 'center';
        g.fillText(String(i + 1), x, y + 3);
        g.textAlign = 'left';
      });
    }
  };

  // --- Rows.
  const rows = () => {
    list.innerHTML = '';
    arr.forEach((q, i) => {
      const row = h('div', { class: 'xw-list-row' + (i === focus ? ' is-on' : ''), 'data-path': `${p}[${i}]` }, h('span', { class: 'xw-dim' }, `${i + 1}.`));
      for (let k = 0; k < n; k++) {
        const box = h('input', { type: 'number', step: '1', value: q[k], title: names[k], 'aria-label': `${o.title} ${i + 1} ${names[k]}` });
        box.addEventListener('focus', () => { focus = i; draw(); });
        box.addEventListener('input', () => { q[k] = intOr(box.value, 0); ctx.changed(); draw(); });
        row.append(box);
      }
      const btn = (label, title, fn) => h('button', { type: 'button', class: 'wb-mini', title, onclick: () => { fn(); ctx.changed(true); } }, label);
      if (i > 0) row.append(btn('↑', 'Move up', () => arr.splice(i - 1, 0, arr.splice(i, 1)[0])));
      if (i < arr.length - 1) row.append(btn('↓', 'Move down', () => arr.splice(i + 1, 0, arr.splice(i, 1)[0])));
      row.append(btn('Copy', 'Duplicate', () => arr.splice(i + 1, 0, q.slice())));
      row.append(btn('×', 'Remove', () => arr.splice(i, 1)));
      list.append(row);
    });
    list.append(h('div', {}, h('button', { type: 'button', class: 'wb-mini', onclick: () => { arr.push(o.make()); ctx.changed(true); } }, `+ ${o.kind === 'rect' ? 'Blocker' : 'Point'}`),
      h('span', { class: 'xw-hint' }, o.kind === 'rect' ? ' or drag on the pad' : ' or click the pad')));
  };

  // --- Pad input.
  const at = (ev) => {
    const r = cv.getBoundingClientRect();
    return [(ev.clientX - r.left) * (cv.width / r.width), (ev.clientY - r.top) * (cv.height / r.height)];
  };
  const snap = (x) => Math.round(x);
  let drag = null;
  cv.addEventListener('pointerdown', (ev) => {
    if (ev.button !== 0) return;
    ev.preventDefault();
    const v = view();
    const [px, py] = at(ev);
    const [wx, wy] = v.fromPx(px, py);
    let hit = -1;
    if (n === 2) {
      arr.forEach((q, i) => { const [x, y] = v.toPx(q[0], q[1]); if (Math.hypot(x - px, y - py) <= 8) hit = i; });
    } else {
      arr.forEach((q, i) => { if (wx >= Math.min(q[0], q[2]) && wx <= Math.max(q[0], q[2]) && wy >= Math.min(q[1], q[3]) && wy <= Math.max(q[1], q[3])) hit = i; });
    }
    if (hit >= 0) {
      focus = hit;
      drag = { move: hit, from: [wx, wy], orig: arr[hit].slice() };
    } else if (n === 2) {
      // A new point goes into the leg of the loop it lengthens least.
      const q = [snap(wx), snap(wy)];
      let at = arr.length;
      if (arr.length > 2) {
        let best = Infinity;
        arr.forEach((a, i) => {
          const b = arr[(i + 1) % arr.length];
          const cost = Math.hypot(q[0] - a[0], q[1] - a[1]) + Math.hypot(q[0] - b[0], q[1] - b[1]) - Math.hypot(b[0] - a[0], b[1] - a[1]);
          if (cost < best) { best = cost; at = i + 1; }
        });
      }
      arr.splice(at, 0, q);
      focus = at;
      drag = { move: focus, from: [wx, wy], orig: arr[focus].slice(), added: true };
      ctx.changed();
    } else {
      arr.push([snap(wx), snap(wy), snap(wx), snap(wy)]);
      focus = arr.length - 1;
      drag = { grow: focus };
      ctx.changed();
    }
    draw();
  });
  const onMove = (ev) => {
    if (!drag || !cv.isConnected) return;
    const v = view();
    const [wx, wy] = v.fromPx(...at(ev));
    if (drag.grow != null) {
      const q = arr[drag.grow];
      q[2] = snap(wx); q[3] = snap(wy);
    } else {
      const q = arr[drag.move];
      const dx = snap(wx - drag.from[0]), dy = snap(wy - drag.from[1]);
      for (let k = 0; k < n; k++) q[k] = drag.orig[k] + (k % 2 ? dy : dx);
    }
    drag.moved = true;
    ctx.changed();
    draw();
  };
  const onUp = () => {
    if (!drag) return;
    const d = drag;
    drag = null;
    if (d.grow != null) {
      const q = arr[d.grow];
      arr[d.grow] = [Math.min(q[0], q[2]), Math.min(q[1], q[3]), Math.max(q[0], q[2]), Math.max(q[1], q[3])];
    }
    if (d.moved || d.added || d.grow != null) ctx.changed(true);
    else rows();
  };
  // One pair of document listeners per pad; a re-render drops the pad and its
  // listeners go quiet (`cv.isConnected`), and are removed on the next pointerup.
  const up = () => { onUp(); if (!cv.isConnected) { document.removeEventListener('pointermove', onMove); document.removeEventListener('pointerup', up); } };
  document.addEventListener('pointermove', onMove);
  document.addEventListener('pointerup', up);

  rows();
  parent.append(wrap);
  requestAnimationFrame(draw);
  draw();
  return wrap;
}

/** The painter's colours for a map's tiles, for a pad over a map. */
export function mapBackground(map) {
  const legend = (map.tiles && map.tiles.legend) || {};
  const rows = (map.tiles && map.tiles.rows) || [];
  return (g, v) => {
    rows.forEach((row, r) => {
      [...row].forEach((ch, c) => {
        if (ch === ' ') return;
        const st = legend[ch];
        g.fillStyle = !st ? '#d04040' : st.walls === 'all' ? '#3a3f47' : '#6b8f71';
        const [x, y] = v.toPx(c - 0.5, r - 0.5);
        g.fillRect(x, y, v.s + 0.5, v.s + 0.5);
      });
    });
  };
}
