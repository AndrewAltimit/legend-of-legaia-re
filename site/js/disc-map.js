/* Disc map: a zoomable treemap of the disc image's sectors.
 *
 * Reads `disc-map.json` (written by `asset disc-map <disc.bin> --out
 * site/disc-map.json` - layout metadata only) and, when present, the
 * generator's `scenes.json` for CDNAME display names. Builds one tree:
 *
 *   disc -> ISO9660 regions (directories group their files)
 *        -> PROT.DAT / DMY.DAT: header rows, then CDNAME blocks -> entries
 *
 * Area is proportional to sector count. The layout is an *ordered*
 * squarified treemap: children keep disc (LBA) order, which is what makes a
 * box's position mean something, at a small cost in aspect ratio.
 */
(function () {
  'use strict';

  const SECTOR = 2048;
  const MAX_DEPTH = 3;          // levels drawn below the focus
  const SVGNS = 'http://www.w3.org/2000/svg';

  const FAMILIES = [
    ['scene',   'Scene data', 'field maps, bundles, scripts, DATA_FIELD streams'],
    ['audio',   'Audio', 'VAB banks, SEQ music, CD-XA streams'],
    ['model',   'Models + animation', 'TMD streams, ANM, battle files, LZS containers'],
    ['code',    'Code', 'the executable and the overlay images'],
    ['texture', 'Textures', 'TIMs and texture packs'],
    ['movie',   'Movies', 'STR / MDEC FMV'],
    ['effect',  'Battle effects', 'effect bundles, summon side-band'],
    ['system',  'System files', 'SYSTEM.CNF, CDNAME.TXT'],
    ['archive', 'Archive header', 'PROT / DMY header and TOC'],
    ['iso',     'ISO9660 + padding', 'system area, directory extents, unclaimed sectors'],
    ['filler',  'Filler', 'pochi slots, zero-filled and constant entries'],
    ['unknown', 'Unidentified', 'no named detector claims these bytes yet'],
  ];
  const FAMILY_LABEL = Object.fromEntries(FAMILIES.map(f => [f[0], f[1]]));

  const esc = window.escapeHtml;
  const $ = id => document.getElementById(id);

  let root = null;
  let focus = null;
  let selected = null;
  let famFilter = null;
  let query = '';
  let matchIds = null;   // Set of node ids matching the search (with ancestors)
  let matchList = [];
  const byId = new Map();
  let inkCache = {};

  /* ---------------- tree ---------------- */

  function node(id, parent, props) {
    const n = Object.assign({ id, parent, children: null, sectors: 0 }, props);
    byId.set(id, n);
    if (parent) (parent.children || (parent.children = [])).push(n);
    return n;
  }

  function finish(n) {
    if (!n.children) {
      n.fam = { [n.family]: n.sectors };
      return n;
    }
    n.children.sort((a, b) => a.lba - b.lba);
    let s = 0;
    const fam = {};
    for (const c of n.children) {
      finish(c);
      s += c.sectors;
      for (const k in c.fam) fam[k] = (fam[k] || 0) + c.fam[k];
    }
    n.sectors = s;
    n.fam = fam;
    let best = null;
    for (const k in fam) if (!best || fam[k] > fam[best]) best = k;
    n.family = best;
    return n;
  }

  function pad4(i) { return String(i).padStart(4, '0'); }

  function buildTree(map, scenes) {
    const sceneByLabel = new Map((scenes || []).map(s => [s.label, s]));
    root = node('disc', null, {
      name: map.volume_id || 'Disc image', kind: 'disc', lba: 0,
      what: 'The whole Mode 2/2352 image', doc: 'formats/disc.html',
    });
    const dirs = new Map();
    const archives = new Map((map.archives || []).map(a => [a.path, a]));
    for (const r of map.regions) {
      let parent = root;
      const slash = r.path.lastIndexOf('/');
      if (r.kind === 'dir' && r.path !== '/') {
        const d = node('disc/' + r.path + '/', root, {
          name: r.path + '/', kind: 'dirgroup', lba: r.lba, what: 'Directory', doc: r.doc,
        });
        dirs.set(r.path, d);
        node('disc/' + r.path + '/.', d, {
          name: r.path + '/ (directory record)', kind: 'dir', lba: r.lba, sectors: r.sectors,
          bytes: r.bytes, family: r.family, what: r.what, doc: r.doc,
        });
        continue;
      }
      if (slash > 0 && dirs.has(r.path.slice(0, slash))) parent = dirs.get(r.path.slice(0, slash));
      const ar = archives.get(r.path);
      const n = node('disc/' + r.path, parent, {
        name: r.path === '/' ? 'Root directory' : r.path, kind: r.kind, lba: r.lba,
        sectors: r.sectors, bytes: r.bytes, family: r.family, what: r.what, doc: r.doc, path: r.path,
      });
      if (ar) buildArchive(n, ar, sceneByLabel);
    }
    finish(root);
  }

  function buildArchive(fileNode, ar, sceneByLabel) {
    const base = fileNode.lba;
    const isProt = /PROT\.DAT$/i.test(ar.path);
    fileNode.archive = ar.path;
    fileNode.children = [];
    for (const h of ar.head) {
      node(fileNode.id + '/h:' + h.lba, fileNode, {
        name: ar.path + ' ' + h.name, kind: 'head', lba: base + h.lba, rel: h.lba,
        sectors: h.sectors, family: h.family, cls: h.class, doc: h.doc,
        what: h.class === 'toc' ? 'Archive header and TOC' : 'Raw TOC row below extraction entry 0',
      });
    }
    let group = null;
    let groupKey = null;
    for (const e of ar.entries) {
      let parent = fileNode;
      if (e.block != null && ar.blocks) {
        if (e.block !== groupKey) {
          const b = ar.blocks[e.block];
          const sc = sceneByLabel.get(b.label);
          group = node(fileNode.id + '/b:' + e.block, fileNode, {
            name: b.label, kind: 'block', lba: base + e.lba, raw: b.raw,
            display: sc ? sc.display : null, category: sc ? sc.category : null,
            what: 'CDNAME block' + (sc ? ' - ' + sc.display : ''), doc: 'formats/cdname.html',
          });
          groupKey = e.block;
        }
        parent = group;
      }
      node(fileNode.id + '/e:' + e.i, parent, {
        name: (isProt ? 'PROT ' : 'DMY ') + pad4(e.i), kind: 'entry', index: e.i, isProt,
        lba: base + e.lba, rel: e.lba, sectors: e.sectors, family: e.family, cls: e.class,
        doc: e.doc, slot: e.slot, block: parent.kind === 'block' ? parent : null,
        overlay: e.overlay || null, known: e.known || null,
        what: e.known || (e.overlay ? 'Overlay image: ' + e.overlay.split('@')[0] : e.class.replace(/_/g, ' ')),
      });
    }
    for (const t of ar.tail || []) {
      node(fileNode.id + '/t:' + t.lba, fileNode, {
        name: ar.path + ' tail', kind: 'head', lba: base + t.lba, rel: t.lba,
        sectors: t.sectors, family: t.family, cls: t.class, doc: fileNode.doc,
        what: 'Sectors past the last TOC entry - no TOC row addresses them',
      });
    }
  }

  /* ---------------- layout ---------------- */

  /* Ordered squarify: rows are filled greedily in the given order. */
  function squarify(items, x, y, w, h) {
    const out = [];
    const total = items.reduce((s, it) => s + it.sectors, 0);
    if (!total || w <= 0 || h <= 0) return out;
    const scale = (w * h) / total;
    let i = 0;
    while (i < items.length) {
      const short = Math.min(w, h);
      let row = [];
      let rowSum = 0;
      let worst = Infinity;
      while (i < items.length) {
        const a = items[i].sectors * scale;
        const s = rowSum + a;
        let mx = 0, mn = Infinity;
        for (const r of row) { mx = Math.max(mx, r.sectors * scale); mn = Math.min(mn, r.sectors * scale); }
        mx = Math.max(mx, a); mn = Math.min(mn, a);
        const wr = Math.max((short * short * mx) / (s * s), (s * s) / (short * short * mn));
        if (row.length && wr > worst) break;
        worst = wr;
        row.push(items[i]);
        rowSum = s;
        i++;
      }
      const thick = rowSum / short;
      let off = 0;
      for (const it of row) {
        const len = (it.sectors * scale) / thick;
        if (w >= h) out.push([it, x, y + off, thick, len]);
        else out.push([it, x + off, y, len, thick]);
        off += len;
      }
      if (w >= h) { x += thick; w -= thick; } else { y += thick; h -= thick; }
    }
    return out;
  }

  /* ---------------- colour ---------------- */

  function famColor(f) {
    return 'var(--dm-' + (f || 'unknown') + ')';
  }

  function refreshInk() {
    inkCache = {};
    const cs = getComputedStyle($('dm-root'));
    for (const [f] of FAMILIES) {
      const v = cs.getPropertyValue('--dm-' + f).trim();
      const m = /^#?([0-9a-f]{6})$/i.exec(v);
      if (!m) { inkCache[f] = '#ffffff'; continue; }
      const n = parseInt(m[1], 16);
      const lin = c => { c /= 255; return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
      const L = 0.2126 * lin(n >> 16) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255);
      // Pick the ink with the better contrast against the fill.
      inkCache[f] = (L + 0.05) / 0.05 > 1.05 / (L + 0.05) ? '#0b0d11' : '#ffffff';
    }
  }

  /* ---------------- render ---------------- */

  function svgEl(tag, attrs) {
    const el = document.createElementNS(SVGNS, tag);
    for (const k in attrs) el.setAttribute(k, attrs[k]);
    return el;
  }

  function nodeDimmed(n) {
    if (matchIds && !matchIds.has(n.id)) return true;
    if (famFilter && !(n.fam && n.fam[famFilter])) return true;
    return false;
  }

  function fitText(s, w, px) {
    const max = Math.floor((w - 6) / (px * 0.62));
    if (max < 3) return null;
    return s.length > max ? s.slice(0, Math.max(1, max - 1)) + '…' : s;
  }

  function boxLabel(n) {
    if (n.kind === 'entry') return pad4(n.index) + ' ' + (n.known || (n.overlay ? n.overlay.split('@')[0] : n.cls));
    if (n.kind === 'block') return n.name + (n.display ? ' · ' + n.display : '');
    return n.name;
  }

  function render() {
    const svg = $('dm-svg');
    const stage = $('dm-stage');
    const W = Math.max(280, Math.floor(stage.clientWidth));
    const H = W < 600 ? Math.round(W * 1.2) : Math.round(Math.max(380, Math.min(W * 0.66, 780)));
    svg.setAttribute('viewBox', '0 0 ' + W + ' ' + H);
    svg.setAttribute('width', W);
    svg.setAttribute('height', H);
    svg.textContent = '';
    const frag = document.createDocumentFragment();

    function draw(n, x, y, w, h, depth) {
      const kids = squarify(n.children, x, y, w, h);
      for (const [c, cx, cy, cw, ch] of kids) {
        if (cw < 0.5 || ch < 0.5) continue;
        const expand = c.children && depth < MAX_DEPTH && cw >= 26 && ch >= 22;
        if (expand) {
          const g = svgEl('rect', { x: cx + 0.5, y: cy + 0.5, width: Math.max(0, cw - 1), height: Math.max(0, ch - 1), class: 'dm-group', 'data-id': c.id });
          if (nodeDimmed(c)) g.classList.add('is-dim');
          frag.appendChild(g);
          const hdr = ch > 40 && cw > 56 ? 16 : 3;
          if (hdr > 3) {
            const label = fitText(boxLabel(c), cw - 40, 11);
            if (label) {
              const t = svgEl('text', { x: cx + 5, y: cy + 12, class: 'dm-glabel' });
              t.textContent = label;
              const pct = fitText(' ' + fmtSize(c.sectors), cw - 8 - label.length * 6.8, 11);
              if (pct) {
                const ts = svgEl('tspan', { class: 'dm-gsize' });
                ts.textContent = pct;
                t.appendChild(ts);
              }
              if (nodeDimmed(c)) t.classList.add('is-dim');
              frag.appendChild(t);
            }
          }
          draw(c, cx + 2, cy + hdr, cw - 4, ch - hdr - 2, depth + 1);
        } else {
          const r = svgEl('rect', { x: cx, y: cy, width: cw, height: ch, class: 'dm-leaf', fill: famColor(c.family), 'data-id': c.id });
          if (nodeDimmed(c)) r.classList.add('is-dim');
          if (selected && selected.id === c.id) r.classList.add('is-sel');
          frag.appendChild(r);
          if (cw > 34 && ch > 15) {
            const label = fitText(boxLabel(c), cw, 10.5);
            if (label) {
              const t = svgEl('text', { x: cx + 4, y: cy + 12, class: 'dm-llabel', fill: inkCache[c.family] || '#fff' });
              t.textContent = label;
              if (nodeDimmed(c)) t.classList.add('is-dim');
              frag.appendChild(t);
              if (ch > 30 && cw > 46) {
                const t2 = svgEl('text', { x: cx + 4, y: cy + 25, class: 'dm-llabel', fill: inkCache[c.family] || '#fff', 'fill-opacity': 0.8 });
                t2.textContent = fitText(fmtSize(c.sectors), cw, 10.5) || '';
                if (nodeDimmed(c)) t2.classList.add('is-dim');
                frag.appendChild(t2);
              }
            }
          }
        }
      }
    }
    if (focus.children) draw(focus, 1, 1, W - 2, H - 2, 0);
    else {
      const r = svgEl('rect', { x: 1, y: 1, width: W - 2, height: H - 2, class: 'dm-leaf is-sel', fill: famColor(focus.family), 'data-id': focus.id });
      frag.appendChild(r);
    }
    svg.appendChild(frag);
    renderCrumbs();
    renderLegend();
    renderList();
  }

  /* ---------------- side panels ---------------- */

  function fmtSize(sectors) {
    const b = sectors * SECTOR;
    if (b >= 1 << 20) return (b / (1 << 20)).toFixed(b >= 10 << 20 ? 0 : 1) + ' MiB';
    if (b >= 1 << 10) return Math.round(b / 1024) + ' KiB';
    return b + ' B';
  }

  function pathTo(n) {
    const out = [];
    for (let p = n; p; p = p.parent) out.unshift(p);
    return out;
  }

  function renderCrumbs() {
    const el = $('dm-crumbs');
    el.textContent = '';
    const path = pathTo(focus);
    path.forEach((n, k) => {
      if (k) {
        const s = document.createElement('span');
        s.className = 'dm-sep';
        s.textContent = '›';
        el.appendChild(s);
      }
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = n === root ? 'disc' : n.name;
      if (n === focus) b.setAttribute('aria-current', 'true');
      else b.addEventListener('click', () => zoomTo(n));
      el.appendChild(b);
    });
  }

  function renderLegend() {
    const el = $('dm-legend');
    el.textContent = '';
    const total = focus.sectors || 1;
    for (const [f, label, desc] of FAMILIES) {
      const s = (focus.fam && focus.fam[f]) || 0;
      if (!s && famFilter !== f) continue;
      const b = document.createElement('button');
      b.type = 'button';
      b.title = desc + ' - click to highlight';
      b.setAttribute('aria-pressed', famFilter === f ? 'true' : 'false');
      const pct = (100 * s) / total;
      b.innerHTML = '<span class="dm-sw" style="background:' + famColor(f) + '"></span>' +
        '<span>' + esc(label) + '</span>' +
        '<span class="dm-pct">' + (pct >= 1 ? pct.toFixed(0) : pct > 0 ? '<1' : '0') + '%</span>';
      b.addEventListener('click', () => { famFilter = famFilter === f ? null : f; render(); });
      el.appendChild(b);
    }
  }

  function listButton(n, onClick) {
    const b = document.createElement('button');
    b.type = 'button';
    b.innerHTML = '<span class="dm-sw" style="background:' + famColor(n.family) + '"></span>' +
      '<span class="dm-n">' + esc(boxLabel(n)) + '</span><span class="dm-s">' + esc(fmtSize(n.sectors)) + '</span>';
    b.addEventListener('click', onClick);
    b.addEventListener('mouseenter', () => hotlight(n, true));
    b.addEventListener('mouseleave', () => hotlight(n, false));
    return b;
  }

  function hotlight(n, on) {
    const r = $('dm-svg').querySelector('[data-id="' + CSS.escape(n.id) + '"]');
    if (r) r.classList.toggle('is-hot', on);
  }

  function renderList() {
    const el = $('dm-list');
    el.textContent = '';
    if (matchIds) {
      $('dm-list-title').textContent = matchList.length
        ? 'Search matches (' + matchList.length + ')' : 'No matches';
      for (const n of matchList.slice(0, 80)) {
        el.appendChild(listButton(n, () => { zoomTo(n.parent || root); select(n); }));
      }
      return;
    }
    $('dm-list-title').textContent = 'Largest boxes in this view';
    const kids = (focus.children || []).slice().sort((a, b) => b.sectors - a.sectors).slice(0, 40);
    for (const n of kids) el.appendChild(listButton(n, () => (n.children ? zoomTo(n) : select(n))));
  }

  function lbaRange(n) {
    return n.lba + '–' + (n.lba + n.sectors - 1);
  }

  function kvRows(n) {
    const rows = [];
    if (n.kind === 'entry') {
      rows.push(['index', String(n.index) + (n.isProt ? ' (extraction)' : '')]);
      if (n.block) rows.push(['block', n.block.name + (n.slot != null ? ' +' + n.slot : '') + (n.block.display ? ' (' + n.block.display + ')' : '')]);
      rows.push(['class', n.cls]);
      if (n.overlay) rows.push(['overlay', n.overlay.replace('@', ' @ ')]);
    }
    if (n.kind === 'head') rows.push(['class', n.cls]);
    if (n.kind === 'block') {
      rows.push(['define', '#define ' + n.name + ' ' + n.raw]);
      if (n.category) rows.push(['category', n.category]);
      rows.push(['entries', String(n.children.length)]);
    }
    rows.push([n.children ? 'mostly' : 'family', FAMILY_LABEL[n.family] || n.family]);
    rows.push(['disc LBA', lbaRange(n)]);
    if (n.rel != null) rows.push(['in file', 'sector ' + n.rel]);
    rows.push(['size', n.sectors + ' sector' + (n.sectors === 1 ? '' : 's') + ' · ' + fmtSize(n.sectors)]);
    if (n.bytes != null && n.kind === 'file') rows.push(['bytes', n.bytes.toLocaleString('en-US')]);
    if (root.sectors) rows.push(['of disc', ((100 * n.sectors) / root.sectors).toFixed(n.sectors * 1000 > root.sectors ? 1 : 3) + '%']);
    return rows;
  }

  function linksFor(n) {
    const out = [];
    if (n.doc) out.push('<a href="' + esc(n.doc) + '">format docs &rarr;</a>');
    if (n.kind === 'entry' && n.isProt) out.push('<a href="viewer.html?prot=' + n.index + '">open in asset viewer &rarr;</a>');
    if (n.kind === 'block' && n.display) out.push('<a href="viewer.html?scene=' + encodeURIComponent(n.name) + '">scene in viewer &rarr;</a>');
    if (/\.(XA|STR)$/i.test(n.path || '') || n.name === 'XA/' || n.name === 'MOV/') out.push('<a href="media.html">media browser &rarr;</a>');
    return out;
  }

  function select(n) {
    selected = n;
    const el = $('dm-detail');
    const rows = kvRows(n).map(([k, v]) => '<dt>' + esc(k) + '</dt><dd>' + esc(v) + '</dd>').join('');
    const links = linksFor(n);
    el.innerHTML = '<h3>Selected</h3>' +
      '<div class="dm-detail-title">' + esc(n.kind === 'block' ? n.name : boxLabel(n)) + '</div>' +
      '<div class="dm-detail-what">' + esc(n.what || '') + '</div>' +
      '<dl class="dm-kv">' + rows + '</dl>' +
      '<div class="dm-links">' + links.join('') +
      (n.children && n !== focus ? '<button type="button" id="dm-zoom-sel">zoom in</button>' : '') + '</div>';
    const zb = $('dm-zoom-sel');
    if (zb) zb.addEventListener('click', () => zoomTo(n));
    const svg = $('dm-svg');
    svg.querySelectorAll('.is-sel').forEach(r => r.classList.remove('is-sel'));
    const r = svg.querySelector('rect.dm-leaf[data-id="' + CSS.escape(n.id) + '"]');
    if (r) r.classList.add('is-sel');
  }

  function zoomTo(n) {
    focus = n.children ? n : n.parent || root;
    const h = focus === root ? '' : '#' + encodeURIComponent(focus.id.replace(/^disc\//, ''));
    if (location.hash !== h) history.replaceState(null, '', h || location.pathname + location.search);
    render();
    if (!selected || !pathTo(selected).includes(focus)) select(focus);
  }

  /* ---------------- search ---------------- */

  function haystack(n) {
    return [n.name, n.cls, n.what, n.known, n.display, n.overlay, n.index != null ? pad4(n.index) : '',
      n.block ? n.block.name : '', n.block && n.block.display, FAMILY_LABEL[n.family]]
      .filter(Boolean).join(' ').toLowerCase();
  }

  function runSearch(q) {
    query = q.trim().toLowerCase();
    if (!query) { matchIds = null; matchList = []; render(); return; }
    const terms = query.split(/\s+/);
    matchIds = new Set();
    matchList = [];
    for (const n of byId.values()) {
      if (n === root) continue;
      const hs = haystack(n);
      const exactIdx = /^\d+$/.test(query) && n.index === Number(query);
      if (exactIdx || terms.every(t => hs.includes(t))) {
        matchList.push(n);
        for (let p = n; p; p = p.parent) matchIds.add(p.id);
        // A matching group lights its whole subtree.
        (function mark(c) { if (c.children) for (const k of c.children) { matchIds.add(k.id); mark(k); } })(n);
      }
    }
    matchList.sort((a, b) => (a.kind === 'entry') - (b.kind === 'entry') || a.lba - b.lba);
    render();
  }

  /* ---------------- events ---------------- */

  function nodeFromEvent(ev) {
    const t = ev.target.closest && ev.target.closest('[data-id]');
    return t ? byId.get(t.getAttribute('data-id')) : null;
  }

  function showTip(n, ev) {
    const tip = $('dm-tip');
    if (!n) { tip.hidden = true; return; }
    const rows = kvRows(n).filter(r => r[0] !== 'of disc' && r[0] !== 'bytes' && r[0] !== 'entries');
    tip.innerHTML = '<b>' + esc(n.kind === 'block' ? n.name : boxLabel(n)) + '</b><br>' +
      (n.what ? esc(n.what) + '<br>' : '') +
      rows.map(([k, v]) => '<span class="dm-k">' + esc(k) + '</span> ' + esc(v)).join('<br>');
    tip.hidden = false;
    const stage = $('dm-stage').getBoundingClientRect();
    let x = ev.clientX - stage.left + 14;
    let y = ev.clientY - stage.top + 14;
    const tw = tip.offsetWidth, th = tip.offsetHeight;
    if (x + tw > stage.width) x = Math.max(0, ev.clientX - stage.left - tw - 14);
    if (y + th > stage.height) y = Math.max(0, ev.clientY - stage.top - th - 14);
    tip.style.left = x + 'px';
    tip.style.top = y + 'px';
  }

  function onClick(ev) {
    const n = nodeFromEvent(ev);
    if (!n) return;
    // Step one level: the focus's child on the path to the clicked box.
    let step = n;
    while (step.parent && step.parent !== focus) step = step.parent;
    if (step.parent !== focus) return;
    if (step.children) {
      zoomTo(step);
      if (!n.children) select(n);
    } else {
      select(step);
    }
  }

  function init(map, scenes) {
    buildTree(map, scenes);
    focus = root;
    refreshInk();
    const want = decodeURIComponent((location.hash || '').slice(1));
    const target = want && byId.get('disc/' + want);
    if (target) zoomTo(target); else render();
    select(target || root);
    $('dm-status') && ($('dm-status').textContent = '');

    const svg = $('dm-svg');
    svg.addEventListener('mousemove', ev => showTip(nodeFromEvent(ev), ev));
    svg.addEventListener('mouseleave', () => showTip(null));
    svg.addEventListener('click', onClick);
    document.addEventListener('keydown', ev => {
      if (ev.key === 'Escape' && focus.parent && document.activeElement !== $('dm-search')) zoomTo(focus.parent);
    });
    let timer = 0;
    $('dm-search').addEventListener('input', ev => {
      clearTimeout(timer);
      const v = ev.target.value;
      timer = setTimeout(() => runSearch(v), 120);
    });
    let lastW = 0;
    new ResizeObserver(() => {
      const w = Math.floor($('dm-stage').clientWidth);
      if (w !== lastW) { lastW = w; render(); }
    }).observe($('dm-stage'));
    new MutationObserver(() => { refreshInk(); render(); })
      .observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
  }

  async function boot() {
    const status = $('dm-status');
    try {
      const [map, scenes] = await Promise.all([
        fetch('disc-map.json').then(r => { if (!r.ok) throw new Error('disc-map.json: HTTP ' + r.status); return r.json(); }),
        fetch('scenes.json').then(r => (r.ok ? r.json() : null)).catch(() => null),
      ]);
      init(map, scenes);
    } catch (e) {
      if (status) status.textContent = 'Could not load the map: ' + e.message;
    }
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', boot);
  else boot();
})();
