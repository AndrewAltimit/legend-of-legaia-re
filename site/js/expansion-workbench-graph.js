/* Expansion workbench: the Flow tab - the pack's progression as a node
 * graph, with the flow lints drawn on it.
 *
 * The graph is the format crate's (`legaia_expansion_format::flow::graph`,
 * through `ExpansionWorkbench::flow_graph`): maps, quests, challenges,
 * bosses, cutscenes, modes, and the flags and counters between them, with
 * the edges the reference model finds - an entrance or exit into a map, a
 * trigger or choice or hook that starts something, a write to a flag, a
 * gate or objective that reads one, a giver that offers a quest. Lines,
 * NPCs, formations and the like fold into the node that holds them. The
 * same pass marks what the player cannot reach from the entry points and
 * every flow lint finding, which the validator lists as warnings too.
 *
 * The layout is computed here with no library: layers by longest path from
 * the entry node over the graph with its back edges set aside, a few
 * barycentre sweeps to order each layer, SVG for the drawing. A click on a
 * node opens its field; the findings list under the graph does the same.
 */

import { h } from './expansion-workbench-forms.js';
import { kindLabel } from './expansion-workbench-rename.js';

const SVGNS = 'http://www.w3.org/2000/svg';
function s(tag, attrs, ...kids) {
  const el = document.createElementNS(SVGNS, tag);
  for (const [k, v] of Object.entries(attrs || {})) if (v != null) el.setAttribute(k, v);
  for (const c of kids.flat()) if (c != null) el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  return el;
}

const KINDS = ['entry', 'map', 'quest', 'challenge', 'boss', 'cutscene', 'mode', 'flag', 'counter'];
const KIND_NAME = { entry: 'entry', map: 'maps', quest: 'quests', challenge: 'challenges', boss: 'bosses', cutscene: 'cutscenes', mode: 'modes', flag: 'flags', counter: 'counters' };
const ROLE_NAME = { enters: 'enters', leads: 'leads to / starts', sets: 'writes', reads: 'read by (gate, objective)', gives: 'giver offers' };

const NODE_W = 172;
const NODE_H = 26;
const COL_W = 214;
const ROW_H = 34;
const PAD = 16;

let styled = false;
function style() {
  if (styled) return;
  styled = true;
  document.head.append(h('style', {}, `
  .xw-flow-bar { display: flex; flex-wrap: wrap; gap: 0.3rem; align-items: center; margin-bottom: 0.4rem; font-size: 0.86em; }
  .xw-flow-sum { margin: 0.2rem 0 0.5rem; font-size: 0.88em; }
  .xw-flow-wrap { position: relative; overflow: auto; max-height: min(70vh, 46rem); border: 1px solid var(--border); border-radius: 6px; background: var(--bg-card); touch-action: pan-x pan-y; }
  .xw-flow-wrap svg { display: block; max-width: none; height: auto; }
  .xw-fn rect { fill: var(--bg-raised); stroke: var(--border-hi); stroke-width: 1; }
  .xw-fn .xw-fn-k { stroke: none; }
  .xw-fn text { fill: var(--text); font-size: 11.5px; font-family: var(--font-mono); pointer-events: none; }
  .xw-fn .xw-fn-kind { fill: var(--text-dim); font-size: 9px; font-family: inherit; }
  .xw-fn { cursor: pointer; }
  .xw-fn:hover rect.xw-fn-box, .xw-fn:focus rect.xw-fn-box { stroke: var(--accent); stroke-width: 2; }
  .xw-fn:focus { outline: none; }
  .xw-fn.is-unreached rect.xw-fn-box { stroke-dasharray: 4 3; }
  .xw-fn.is-unreached { opacity: 0.55; }
  .xw-fn.is-problem rect.xw-fn-box { stroke: var(--warn); stroke-width: 2; }
  .xw-fn .xw-fn-bad { fill: var(--warn); }
  .xw-fn .xw-fn-badt { fill: #1d1500; font-size: 10px; font-weight: 700; font-family: inherit; }
  .xw-fe { fill: none; stroke-width: 1.3; opacity: 0.55; }
  .xw-fe.r-leads, .xw-fe.r-enters { stroke: var(--accent); }
  .xw-fe.r-gives { stroke: var(--accent-hi); }
  .xw-fe.r-sets { stroke: var(--ok); }
  .xw-fe.r-reads { stroke: var(--text-dim); stroke-dasharray: 4 3; }
  .xw-fe.is-back { stroke-dasharray: 2 4; }
  .xw-flow-wrap.is-hover .xw-fe { opacity: 0.12; }
  .xw-flow-wrap.is-hover .xw-fe.is-hot { opacity: 1; stroke-width: 2; }
  .xw-flow-wrap.is-hover .xw-fn { opacity: 0.35; }
  .xw-flow-wrap.is-hover .xw-fn.is-hot { opacity: 1; }
  .xw-flow-wrap.is-only .xw-fn:not(.is-problem):not(.is-hot) { opacity: 0.25; }
  .xw-fk-entry { fill: #8a8f98; } .xw-fk-map { fill: #3f86d1; } .xw-fk-quest { fill: #c8902a; }
  .xw-fk-challenge { fill: #c0607e; } .xw-fk-boss { fill: #cf4f45; } .xw-fk-cutscene { fill: #8467d0; }
  .xw-fk-mode { fill: #2fa595; } .xw-fk-flag { fill: #6f9a36; } .xw-fk-counter { fill: #a08a2e; }
  .xw-flow-legend { display: flex; flex-wrap: wrap; gap: 0.2rem 0.8rem; margin: 0.4rem 0; font-size: 0.8em; color: var(--text-dim); }
  .xw-flow-legend svg { vertical-align: middle; }
  .xw-chip { display: inline-flex; align-items: center; gap: 0.25rem; padding: 0.05rem 0.45rem; border: 1px solid var(--border); border-radius: 999px; background: var(--bg-code); cursor: pointer; user-select: none; }
  .xw-chip input { margin: 0; }
  .xw-chip i { display: inline-block; width: 0.7em; height: 0.7em; border-radius: 2px; }
  .xw-flow-probs { margin-top: 0.6rem; }
  .xw-flow-probs h4 { margin: 0.4rem 0 0.2rem; font-size: 0.92em; }
  `));
}

const KIND_COLOR = { entry: '#8a8f98', map: '#3f86d1', quest: '#c8902a', challenge: '#c0607e', boss: '#cf4f45', cutscene: '#8467d0', mode: '#2fa595', flag: '#6f9a36', counter: '#a08a2e' };

/** Layout: `{ pos: Map(key -> { x, y, layer }), back: Set(edge index), width, height }`. */
export function layout(nodes, edges) {
  const keys = nodes.map((n) => n.key);
  const idx = new Map(keys.map((k, i) => [k, i]));
  const out = keys.map(() => []);
  edges.forEach((e, i) => { const a = idx.get(e.from); const b = idx.get(e.to); if (a != null && b != null && a !== b) out[a].push([b, i]); });
  // Back edges: a DFS from the entry first, then from every unvisited node
  // in kind order, sets aside an edge into a node still on the stack.
  const state = new Uint8Array(keys.length); // 0 new, 1 on stack, 2 done
  const back = new Set();
  const order = keys.map((_, i) => i).sort((a, b) => (KINDS.indexOf(nodes[a].kind) - KINDS.indexOf(nodes[b].kind)) || keys[a].localeCompare(keys[b]));
  for (const root of order) {
    if (state[root]) continue;
    const stack = [[root, 0]];
    state[root] = 1;
    while (stack.length) {
      const top = stack[stack.length - 1];
      const [v, j] = top;
      if (j < out[v].length) {
        top[1]++;
        const [w, ei] = out[v][j];
        if (state[w] === 1) back.add(ei);
        else if (state[w] === 0) { state[w] = 1; stack.push([w, 0]); }
      } else { state[v] = 2; stack.pop(); }
    }
  }
  // Layers: the breadth-first distance from the entry node, so a column
  // reads as "this far into the pack". What the entry does not reach is
  // laid out from its own sources the same way.
  const layer = new Array(keys.length).fill(-1);
  const bfs = (starts) => {
    const q = starts.filter((v) => layer[v] < 0);
    q.forEach((v) => { layer[v] = 0; });
    for (let i = 0; i < q.length; i++) {
      const v = q[i];
      for (const [w] of out[v]) if (layer[w] < 0) { layer[w] = layer[v] + 1; q.push(w); }
    }
  };
  bfs(keys.map((_, i) => i).filter((i) => nodes[i].kind === 'entry'));
  const indeg = new Array(keys.length).fill(0);
  edges.forEach((e, i) => { if (!back.has(i) && idx.has(e.from) && idx.has(e.to) && e.from !== e.to) indeg[idx.get(e.to)]++; });
  bfs(order.filter((v) => layer[v] < 0 && indeg[v] === 0));
  for (const v of order) if (layer[v] < 0) bfs([v]);
  const nLayers = Math.max(0, ...layer) + 1;
  const rows = Array.from({ length: nLayers }, () => []);
  order.forEach((v) => rows[layer[v]].push(v));
  const preds = keys.map(() => []);
  const succs = keys.map(() => []);
  edges.forEach((e, i) => {
    const a = idx.get(e.from); const b = idx.get(e.to);
    if (a == null || b == null || a === b || back.has(i)) return;
    succs[a].push(b); preds[b].push(a);
  });
  const rank = new Array(keys.length).fill(0);
  const setRanks = () => rows.forEach((r) => r.forEach((v, i) => { rank[v] = i; }));
  setRanks();
  const bary = (v, nb) => (nb[v].length ? nb[v].reduce((t, w) => t + rank[w], 0) / nb[v].length : rank[v]);
  for (let sweep = 0; sweep < 6; sweep++) {
    const down = sweep % 2 === 0;
    const seq = down ? rows.keys() : [...rows.keys()].reverse();
    for (const li of seq) {
      const nb = down ? preds : succs;
      const b = new Map(rows[li].map((v) => [v, bary(v, nb)]));
      rows[li].sort((x, y) => b.get(x) - b.get(y));
      rows[li].forEach((v, i) => { rank[v] = i; });
    }
  }
  const pos = new Map();
  let tallest = 0;
  rows.forEach((r, li) => {
    tallest = Math.max(tallest, r.length);
    r.forEach((v, i) => pos.set(keys[v], { x: PAD + li * COL_W, y: PAD + i * ROW_H, layer: li }));
  });
  return { pos, back, width: PAD * 2 + (nLayers - 1) * COL_W + NODE_W, height: PAD * 2 + Math.max(1, tallest) * ROW_H };
}

const short = (t, n) => (t.length > n ? t.slice(0, n - 1) + '…' : t);

/** The Flow tab. */
export function renderFlow(ctx, el) {
  style();
  let g;
  try { g = ctx.flowGraph(); } catch (e) { el.append(h('div', { class: 'wb-status is-err' }, 'The flow graph could not be built: ' + (e && e.message ? e.message : e))); return; }
  const shown = ctx.sel.flowKinds || (ctx.sel.flowKinds = Object.fromEntries(KINDS.map((k) => [k, true])));
  const nodeProblems = g.nodes.reduce((n, x) => n + x.problems.length, 0);
  const unreached = g.nodes.filter((n) => !n.reached && n.kind !== 'flag' && n.kind !== 'counter').length;
  el.append(h('p', { class: 'xw-dim' },
    'The pack\'s progression: from the retail game into the entrance maps, through exits, triggers, quests, fights and cutscenes, with the pack flags and counters between them. ',
    'A dashed node cannot be reached from the entry points; an outlined one has a finding. Click a node to open its field.'));
  el.append(h('div', { class: 'xw-flow-sum' },
    `${g.nodes.length} node(s), ${g.edges.length} edge(s); `,
    h('strong', {}, `${nodeProblems + g.problems.length} finding(s)`), `, ${unreached} unreachable.`));

  const bar = h('div', { class: 'xw-flow-bar', role: 'toolbar', 'aria-label': 'Flow graph filters' });
  for (const k of KINDS) {
    const n = g.nodes.filter((x) => x.kind === k).length;
    if (!n) continue;
    const cb = h('input', { type: 'checkbox', checked: shown[k] ? true : null, onchange: (ev) => { shown[k] = ev.target.checked; draw(); } });
    bar.append(h('label', { class: 'xw-chip' }, cb, h('i', { style: `background:${KIND_COLOR[k]}` }), `${KIND_NAME[k]} ${n}`));
  }
  const only = h('input', { type: 'checkbox', onchange: () => wrap.classList.toggle('is-only', only.checked) });
  bar.append(h('label', { class: 'xw-chip', title: 'Fade every node without a finding' }, only, 'findings only'));
  let zoom = ctx.sel.flowZoom || 1;
  const setZoom = (z) => { zoom = Math.max(0.3, Math.min(2, z)); ctx.sel.flowZoom = zoom; size(); };
  bar.append(
    h('button', { type: 'button', class: 'wb-mini', 'aria-label': 'Zoom out', onclick: () => setZoom(zoom / 1.25) }, '−'),
    h('button', { type: 'button', class: 'wb-mini', 'aria-label': 'Zoom in', onclick: () => setZoom(zoom * 1.25) }, '+'),
    h('button', { type: 'button', class: 'wb-mini', onclick: () => setZoom(Math.min(1, (wrap.clientWidth - 4) / Math.max(1, lay.width))) }, 'Fit width'));
  el.append(bar);

  const legend = h('div', { class: 'xw-flow-legend' });
  for (const [role, label] of Object.entries(ROLE_NAME)) {
    legend.append(h('span', {}, svgLine(role), ' ', label));
  }
  el.append(legend);

  const wrap = h('div', { class: 'xw-flow-wrap' });
  el.append(wrap);
  let svg = null;
  let lay = { width: 0, height: 0 };
  const size = () => {
    if (!svg) return;
    svg.setAttribute('width', Math.round(lay.width * zoom));
    svg.setAttribute('height', Math.round(lay.height * zoom));
  };
  const jump = (n) => ctx.jump({ path: n.file, field: n.field });

  function draw() {
    wrap.innerHTML = '';
    const nodes = g.nodes.filter((n) => shown[n.kind]);
    const keys = new Set(nodes.map((n) => n.key));
    const edges = g.edges.filter((e) => keys.has(e.from) && keys.has(e.to));
    lay = layout(nodes, edges);
    svg = s('svg', { viewBox: `0 0 ${lay.width} ${lay.height}`, role: 'img', 'aria-label': `Flow graph: ${nodes.length} nodes, ${edges.length} edges` });
    const eLayer = s('g', {});
    const nLayer = s('g', {});
    svg.append(eLayer, nLayer);
    const edgeEls = new Map(); // node key -> [edge els]
    edges.forEach((e, i) => {
      const a = lay.pos.get(e.from); const b = lay.pos.get(e.to);
      const x1 = a.x + NODE_W; const y1 = a.y + NODE_H / 2;
      const x2 = b.x; const y2 = b.y + NODE_H / 2;
      let d;
      if (x2 > x1) {
        const c = Math.max(30, (x2 - x1) / 2);
        d = `M${x1},${y1} C${x1 + c},${y1} ${x2 - c},${y2} ${x2},${y2}`;
      } else {
        // Backwards or same column: loop under the nodes.
        const dip = Math.max(a.y, b.y) + NODE_H + 14 + (i % 4) * 4;
        d = `M${x1},${y1} C${x1 + 50},${dip} ${x2 - 50},${dip} ${x2},${y2}`;
      }
      const p = s('path', { d, class: `xw-fe r-${e.role}${lay.back.has(i) || x2 <= x1 ? ' is-back' : ''}`, 'marker-end': 'url(#xw-arrow)' },
        s('title', {}, `${e.from} ${ROLE_NAME[e.role] || e.role} ${e.to}\n${e.file} ${e.field}`));
      eLayer.append(p);
      for (const k of [e.from, e.to]) { if (!edgeEls.has(k)) edgeEls.set(k, []); edgeEls.get(k).push([p, e]); }
    });
    svg.prepend(s('defs', {}, s('marker', { id: 'xw-arrow', viewBox: '0 0 8 8', refX: 7, refY: 4, markerWidth: 6, markerHeight: 6, orient: 'auto-start-reverse' },
      s('path', { d: 'M0,0 L8,4 L0,8 z', fill: 'var(--text-dim)' }))));
    const nodeEls = new Map();
    for (const n of nodes) {
      const p = lay.pos.get(n.key);
      const cls = ['xw-fn'];
      if (!n.reached && n.kind !== 'flag' && n.kind !== 'counter') cls.push('is-unreached');
      if (n.problems.length) cls.push('is-problem');
      const label = n.kind === 'entry' ? n.id : n.id;
      const tip = [`${kindLabel(n.kind)} ${n.id}`, n.kind === 'flag' || n.kind === 'counter' ? `written ${n.sets}, read ${n.reads}${n.engine ? ' (raised by the engine)' : ''}` : n.reached ? 'reachable' : 'not reachable from the entry points',
        ...n.problems.map((x) => '! ' + x.message)].join('\n');
      const gEl = s('g', { class: cls.join(' '), transform: `translate(${p.x},${p.y})`, tabindex: '0', role: 'button', 'aria-label': tip },
        s('title', {}, tip),
        s('rect', { class: 'xw-fn-box', width: NODE_W, height: NODE_H, rx: 4 }),
        s('rect', { class: `xw-fn-k xw-fk-${n.kind}`, width: 5, height: NODE_H, rx: 2 }),
        s('text', { x: 10, y: 10, class: 'xw-fn-kind' }, n.kind === 'entry' ? 'entry' : kindLabel(n.kind)),
        s('text', { x: 10, y: 21 }, short(label, n.problems.length ? 20 : 23)));
      if (n.problems.length) {
        gEl.append(s('circle', { class: 'xw-fn-bad', cx: NODE_W - 11, cy: NODE_H / 2, r: 8 }),
          s('text', { class: 'xw-fn-badt', x: NODE_W - 11, y: NODE_H / 2 + 3.5, 'text-anchor': 'middle' }, n.problems.length > 9 ? '9+' : n.problems.length));
      }
      gEl.addEventListener('click', () => jump(n));
      gEl.addEventListener('keydown', (ev) => { if (ev.key === 'Enter' || ev.key === ' ') { ev.preventDefault(); jump(n); } });
      const hot = (on) => {
        wrap.classList.toggle('is-hover', on);
        gEl.classList.toggle('is-hot', on);
        for (const [pe, e] of edgeEls.get(n.key) || []) {
          pe.classList.toggle('is-hot', on);
          const other = nodeEls.get(e.from === n.key ? e.to : e.from);
          if (other) other.classList.toggle('is-hot', on);
        }
      };
      gEl.addEventListener('mouseenter', () => hot(true));
      gEl.addEventListener('mouseleave', () => hot(false));
      gEl.addEventListener('focus', () => hot(true));
      gEl.addEventListener('blur', () => hot(false));
      nodeEls.set(n.key, gEl);
      nLayer.append(gEl);
    }
    wrap.append(svg);
    size();
    wrap._nodeEls = nodeEls;
  }
  draw();

  // The findings, as a list that jumps (and works at any width).
  const probs = h('div', { class: 'xw-flow-probs' });
  const all = [...g.problems.map((p) => ({ p, n: null })), ...g.nodes.flatMap((n) => n.problems.map((p) => ({ p, n })))];
  probs.append(h('h4', {}, all.length ? `Findings (${all.length})` : 'No flow findings: every map and quest is reachable and every flag is both written and read.'));
  const byKind = new Map();
  for (const x of all) {
    const k = x.n ? x.n.kind : 'pack';
    if (!byKind.has(k)) byKind.set(k, []);
    byKind.get(k).push(x);
  }
  for (const [k, list] of byKind) {
    probs.append(h('div', { class: 'xw-find-role' }, k === 'pack' ? 'Pack' : KIND_NAME[k] || k));
    for (const { p, n } of list) {
      probs.append(h('button', {
        type: 'button', class: 'xw-issue is-warning',
        onclick: () => ctx.jump({ path: p.file, field: p.field }),
        onmouseenter: () => { const e = n && wrap._nodeEls && wrap._nodeEls.get(n.key); if (e) e.dispatchEvent(new Event('mouseenter')); },
        onmouseleave: () => { const e = n && wrap._nodeEls && wrap._nodeEls.get(n.key); if (e) e.dispatchEvent(new Event('mouseleave')); },
      }, h('code', {}, `${p.file}${p.field ? ' ' + p.field : ''} `), p.message));
    }
  }
  el.append(probs);
}

function svgLine(role) {
  const el = s('svg', { width: 28, height: 8, 'aria-hidden': 'true' }, s('path', { d: 'M1,4 L27,4', class: `xw-fe r-${role}`, style: 'opacity:1' }));
  return el;
}
