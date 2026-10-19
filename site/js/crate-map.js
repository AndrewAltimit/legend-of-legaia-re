/* crate-map.js - the interactive crate map (crates.html).
 *
 * Data: crate-map.json, written by scripts/ci/gen-crate-map.py from the
 * workspace's own manifests, CLAUDE.md's crate tables and the layering table.
 * Nothing here knows a crate name: the bands, edges, walkthrough and host
 * notes all come from the JSON.
 *
 * Interaction model:
 *   - hover / focus a chip: light the crates it uses and the crates using it,
 *     and draw those edges (transient; restores the selection on leave);
 *   - click / Enter: select it (sticky), fill the side card, set #crate=<id>;
 *   - Escape: clear the selection or the walkthrough trace;
 *   - track buttons + search box dim what they exclude, never re-flow;
 *   - "Disc to pixels" steps light their crates; "Trace on the map" carries
 *     the highlight and an animated path onto the map.
 * Classic script (no module syntax); waits for DOMContentLoaded.
 */
(function () {
  'use strict';

  var esc = window.escapeHtml;
  var REDUCED = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  var SVGNS = 'http://www.w3.org/2000/svg';

  var data = null;
  var chips = {};            // id -> button
  var activeTracks = {};     // track id -> bool
  var query = '';
  var selected = null;
  var hovered = null;
  var walkIndex = 0;
  var walkTrace = false;     // walkthrough highlight carried onto the map
  var playTimer = null;

  function $(id) { return document.getElementById(id); }

  function closure(start, key) {
    var seen = {}, stack = [start];
    while (stack.length) {
      var c = stack.pop();
      var next = data.crates[c][key];
      for (var i = 0; i < next.length; i++) {
        var n = next[i];
        if (!seen[n] && data.crates[n]) { seen[n] = true; stack.push(n); }
      }
    }
    return seen;
  }

  function trackTitle(id) {
    for (var i = 0; i < data.tracks.length; i++) if (data.tracks[i].id === id) return data.tracks[i].title;
    return id;
  }
  function bandOf(id) {
    for (var i = 0; i < data.bands.length; i++) if (data.bands[i].id === id) return data.bands[i];
    return null;
  }

  /* ---------- Build ---------- */

  function buildTracks() {
    var host = $('cm-tracks');
    data.tracks.forEach(function (t) {
      activeTracks[t.id] = true;
      var b = document.createElement('button');
      b.type = 'button';
      b.className = 't-' + t.id;
      b.setAttribute('aria-pressed', 'true');
      b.title = t.blurb;
      b.innerHTML = '<span class="cm-sw" aria-hidden="true"></span>' + esc(t.title);
      b.addEventListener('click', function () {
        activeTracks[t.id] = !activeTracks[t.id];
        b.setAttribute('aria-pressed', activeTracks[t.id] ? 'true' : 'false');
        applyFilter();
      });
      host.appendChild(b);
    });
  }

  function buildBands() {
    var host = $('cm-bands');
    host.innerHTML = '';
    data.bands.forEach(function (band) {
      var n = 0;
      band.rows.forEach(function (r) { n += r.crates.length; });
      var el = document.createElement('div');
      el.className = 'cm-band';
      el.setAttribute('role', 'group');
      el.setAttribute('aria-label', band.title);
      var head = '<div class="cm-band-head"><div class="cm-bt">' + esc(band.title) +
        ' <span class="cm-count">' + n + '</span></div><p>' + esc(band.blurb) + '</p></div>';
      el.innerHTML = head + '<div class="cm-rows"></div>';
      var rows = el.querySelector('.cm-rows');
      band.rows.forEach(function (r) {
        var row = document.createElement('div');
        row.className = 'cm-row';
        if (r.label) {
          var lab = document.createElement('span');
          lab.className = 'cm-rank';
          lab.textContent = r.label;
          row.appendChild(lab);
        }
        r.crates.forEach(function (id) {
          var c = data.crates[id];
          var b = document.createElement('button');
          b.type = 'button';
          b.className = 'cm-chip t-' + c.track;
          b.dataset.crate = id;
          b.textContent = id;
          b.setAttribute('aria-label', id + ' - ' + trackTitle(c.track) + ', uses ' + c.deps.length +
            ', used by ' + c.dependents.length);
          b.addEventListener('mouseenter', function () { hover(id); });
          b.addEventListener('mouseleave', function () { hover(null); });
          b.addEventListener('focus', function () { hover(id); });
          b.addEventListener('blur', function () { hover(null); });
          b.addEventListener('click', function () { select(selected === id ? null : id, true); });
          chips[id] = b;
          row.appendChild(b);
        });
        rows.appendChild(row);
      });
      host.appendChild(el);
    });
  }

  /* ---------- Highlight + edges ---------- */

  function clearMarks() {
    for (var id in chips) chips[id].classList.remove('is-hot', 'is-sel', 'is-uses', 'is-usedby', 'is-far', 'is-walk');
  }

  function render() {
    clearMarks();
    var root = $('cm-root');
    var svg = $('cm-edges');
    svg.innerHTML = '';
    var focus = hovered || selected;
    if (walkTrace && !hovered) return renderWalkTrace();
    if (!focus) { root.classList.remove('is-focus'); return; }
    root.classList.add('is-focus');
    var c = data.crates[focus];
    chips[focus].classList.add('is-hot');
    if (selected) chips[selected].classList.add('is-sel');
    var transitive = $('cm-transitive').checked;
    if (transitive) {
      var far1 = closure(focus, 'deps'), far2 = closure(focus, 'dependents');
      for (var a in far1) chips[a].classList.add('is-far');
      for (var b in far2) chips[b].classList.add('is-far');
    }
    c.deps.forEach(function (d) { if (chips[d]) { chips[d].classList.add('is-uses'); chips[d].classList.remove('is-far'); } });
    c.dependents.forEach(function (d) { if (chips[d]) { chips[d].classList.add('is-usedby'); chips[d].classList.remove('is-far'); } });
    var frame = frameRect();
    c.deps.forEach(function (d) { if (chips[d]) edge(svg, frame, chips[focus], chips[d], 'uses'); });
    c.dependents.forEach(function (d) { if (chips[d]) edge(svg, frame, chips[d], chips[focus], 'usedby'); });
  }

  function renderWalkTrace() {
    var root = $('cm-root');
    root.classList.add('is-focus');
    var svg = $('cm-edges');
    var step = data.walk[walkIndex];
    step.crates.forEach(function (id) { if (chips[id]) chips[id].classList.add('is-walk'); });
    if (walkIndex > 0) {
      var frame = frameRect();
      var prev = data.walk[walkIndex - 1].crates;
      prev.forEach(function (id) { if (chips[id]) chips[id].classList.add('is-hot'); });
      // Join each previous-step crate to each current-step crate it is
      // related to; when no edge relates them, join first to first so the
      // path stays continuous.
      var drawn = 0;
      prev.forEach(function (p) {
        step.crates.forEach(function (q) {
          var related = data.crates[q].deps.indexOf(p) >= 0 || data.crates[p].deps.indexOf(q) >= 0;
          if (related && chips[p] && chips[q]) { edge(svg, frame, chips[p], chips[q], 'walk'); drawn++; }
        });
      });
      if (!drawn && chips[prev[0]] && chips[step.crates[0]]) edge(svg, frame, chips[prev[0]], chips[step.crates[0]], 'walk');
    }
  }

  function frameRect() {
    var stage = $('cm-stage');
    var r = stage.getBoundingClientRect();
    var svg = $('cm-edges');
    svg.setAttribute('viewBox', '0 0 ' + Math.max(1, r.width) + ' ' + Math.max(1, r.height));
    return r;
  }

  function clamp(v, lo, hi) { return Math.max(lo, Math.min(hi, v)); }

  function edge(svg, frame, from, to, cls) {
    var a = from.getBoundingClientRect(), b = to.getBoundingClientRect();
    // Each end leaves its chip at the point nearest the other end, so a
    // crate with many edges fans them along its edge instead of one spot.
    var acx = a.left + a.width / 2, bcx = b.left + b.width / 2;
    var ax = clamp(bcx, a.left + 6, a.right - 6) - frame.left;
    var bx = clamp(acx, b.left + 6, b.right - 6) - frame.left;
    var ay, by, d;
    if (Math.abs(a.top - b.top) < 4) {
      // Same row: arc over the chips.
      ay = a.top - frame.top; by = b.top - frame.top;
      var lift = Math.min(40, 14 + Math.abs(ax - bx) / 6);
      d = 'M' + ax + ',' + ay + ' C' + ax + ',' + (ay - lift) + ' ' + bx + ',' + (by - lift) + ' ' + bx + ',' + by;
    } else {
      var down = b.top > a.top;
      ay = (down ? a.bottom : a.top) - frame.top;
      by = (down ? b.top : b.bottom) - frame.top;
      var my = (ay + by) / 2;
      d = 'M' + ax + ',' + ay + ' C' + ax + ',' + my + ' ' + bx + ',' + my + ' ' + bx + ',' + by;
    }
    var p = document.createElementNS(SVGNS, 'path');
    p.setAttribute('d', d);
    p.setAttribute('class', cls);
    svg.appendChild(p);
  }

  /* ---------- Filter ---------- */

  function matches(id) {
    if (!query) return true;
    var c = data.crates[id];
    if (id.indexOf(query) >= 0 || c.package.indexOf(query) >= 0) return true;
    for (var i = 0; i < c.modules.length; i++) if (c.modules[i].indexOf(query) >= 0) return true;
    for (var j = 0; j < c.bins.length; j++) if (c.bins[j].indexOf(query) >= 0) return true;
    return false;
  }

  function applyFilter() {
    var shown = 0, edges = 0, total = 0;
    var vis = {};
    for (var id in chips) {
      total++;
      var on = activeTracks[data.crates[id].track] && matches(id);
      chips[id].classList.toggle('is-off', !on);
      if (on) { vis[id] = true; shown++; }
    }
    for (var id2 in vis) data.crates[id2].deps.forEach(function (d) { if (vis[d]) edges++; });
    $('cm-stats').textContent = (shown === total ? total + ' crates' : shown + ' of ' + total + ' crates') +
      ' · ' + edges + ' edges';
  }

  /* ---------- Card ---------- */

  function miniList(ids) {
    if (!ids.length) return '<span class="cm-none">none</span>';
    return '<div class="cm-mini">' + ids.map(function (d) {
      var c = data.crates[d];
      return '<button type="button" class="t-' + (c ? c.track : 'engine') + '" data-go="' + esc(d) + '">' + esc(d) + '</button>';
    }).join('') + '</div>';
  }

  function cardFor(id) {
    var c = data.crates[id];
    var band = bandOf(c.band);
    var h = '<div class="cm-name">' + esc(id) + '</div><div class="cm-pkg">' + esc(c.package) + '</div>';
    h += '<div class="cm-tags"><span class="cm-tag t-' + c.track + '">' + esc(trackTitle(c.track)) + '</span>';
    h += '<span class="cm-tag">' + esc(band ? band.title : c.band) + (c.rank != null ? ', rank ' + c.rank : '') + '</span></div>';
    if (c.scope) h += '<div class="cm-scope">' + c.scope + '</div>';
    if (c.role) h += '<div class="cm-h">Layering role</div><div class="cm-coupling">' + esc(c.role) + '</div>';
    if (c.bins.length) h += '<div class="cm-h">Binaries</div><div class="cm-mods">' + c.bins.map(esc).join(', ') + '</div>';
    h += '<div class="cm-h">Uses (' + c.deps.length + ')</div>' + miniList(c.deps);
    h += '<div class="cm-h">Used by (' + c.dependents.length + ')</div>' + miniList(c.dependents);
    if (c.dev_deps.length) h += '<div class="cm-h">Tests also use</div>' + miniList(c.dev_deps);
    var coup = data.couplings.filter(function (k) { return k.from === id || k.to === id; });
    if (coup.length) {
      h += '<div class="cm-h">Edges with a recorded reason</div>';
      coup.forEach(function (k) {
        h += '<p class="cm-coupling"><code>' + esc(k.from) + '</code> → <code>' + esc(k.to) + '</code>: ' + esc(k.reason) + '</p>';
      });
    }
    if (c.modules.length) {
      var mods = c.modules.map(function (m) { return esc(m); }).join(' · ');
      if (c.modules.length > 14) {
        h += '<details><summary class="cm-h">Public modules (' + c.modules.length + ')</summary><div class="cm-mods">' + mods + '</div></details>';
      } else {
        h += '<div class="cm-h">Public modules</div><div class="cm-mods">' + mods + '</div>';
      }
    }
    h += '<div class="cm-links"><a href="' + esc(c.readme) + '" target="_blank" rel="noopener">README</a>' +
      '<button type="button" data-clear="1">Clear</button></div>';
    return h;
  }

  function walkCard() {
    var step = data.walk[walkIndex];
    return '<div class="cm-pkg">Disc to pixels · step ' + (walkIndex + 1) + ' of ' + data.walk.length + '</div>' +
      '<div class="cm-name">' + esc(step.title) + '</div><div class="cm-scope">' + esc(step.text) + '</div>' +
      miniList(step.crates) +
      '<div class="cm-links"><button type="button" data-walk="-1"' + (walkIndex === 0 ? ' disabled' : '') + '>Previous</button>' +
      '<button type="button" data-walk="1"' + (walkIndex === data.walk.length - 1 ? ' disabled' : '') + '>Next</button>' +
      '<button type="button" data-clear="1">End trace</button></div>';
  }

  var EMPTY_CARD = null;
  function renderCard() {
    var card = $('cm-card');
    if (EMPTY_CARD === null) EMPTY_CARD = card.innerHTML;
    if (walkTrace) card.innerHTML = walkCard();
    else if (selected) card.innerHTML = cardFor(selected);
    else card.innerHTML = EMPTY_CARD;
  }

  function onCardClick(e) {
    var t = e.target.closest('[data-go],[data-clear],[data-walk]');
    if (!t) return;
    if (t.dataset.go) { walkTrace = false; select(t.dataset.go, false); if (chips[t.dataset.go]) chips[t.dataset.go].focus({ preventScroll: true }); }
    else if (t.dataset.clear) { walkTrace = false; select(null, false); }
    else if (t.dataset.walk) { setWalk(walkIndex + parseInt(t.dataset.walk, 10), true); }
  }

  /* ---------- Selection ---------- */

  function hover(id) {
    hovered = id;
    render();
  }

  function select(id, fromChip) {
    selected = id && data.crates[id] ? id : null;
    if (selected) walkTrace = false;
    try {
      history.replaceState(null, '', selected ? '#crate=' + selected : location.pathname + location.search);
    } catch (e) { /* file:// or sandboxed */ }
    render();
    renderCard();
    placeCard();
    if (selected && fromChip && isNarrow()) {
      $('cm-card').scrollIntoView({ behavior: REDUCED ? 'auto' : 'smooth', block: 'nearest' });
    }
  }

  /* Below the two-column breakpoint the card would sit after every band, a
   * long scroll from the chip that filled it. There it moves in after the
   * selected crate's band instead; at desktop width it stays in its column. */
  function isNarrow() { return window.matchMedia('(max-width: 1100px)').matches; }
  function placeCard() {
    var card = $('cm-card');
    var focus = selected || (walkTrace ? data.walk[walkIndex].crates[0] : null);
    if (isNarrow() && focus && chips[focus]) {
      var band = chips[focus].closest('.cm-band');
      if (band && band.nextSibling !== card) band.parentNode.insertBefore(card, band.nextSibling);
    } else if (card.parentNode !== $('cm-body')) {
      $('cm-body').appendChild(card);
    }
    render();
  }

  /* ---------- Walkthrough ---------- */

  function buildWalk() {
    var list = $('cm-steps');
    data.walk.forEach(function (step, i) {
      var li = document.createElement('li');
      var b = document.createElement('button');
      b.type = 'button';
      b.innerHTML = '<span class="cm-n">' + (i + 1) + '</span>' + esc(step.title);
      b.addEventListener('click', function () { stopPlay(); setWalk(i, false); });
      li.appendChild(b);
      list.appendChild(li);
    });
    $('cm-step-panel').addEventListener('click', function (e) {
      var t = e.target.closest('[data-go],[data-act]');
      if (!t) return;
      if (t.dataset.go) {
        walkTrace = false;
        select(t.dataset.go, false);
        $('map').scrollIntoView({ behavior: REDUCED ? 'auto' : 'smooth' });
        return;
      }
      var act = t.dataset.act;
      if (act === 'prev') { stopPlay(); setWalk(walkIndex - 1, false); }
      else if (act === 'next') { stopPlay(); setWalk(walkIndex + 1, false); }
      else if (act === 'play') { if (playTimer) stopPlay(); else startPlay(); renderStepPanel(); }
      else if (act === 'trace') {
        stopPlay();
        selected = null;
        walkTrace = true;
        render(); renderCard(); placeCard();
        $('map').scrollIntoView({ behavior: REDUCED ? 'auto' : 'smooth' });
      }
    });
    setWalk(0, false);
  }

  function setWalk(i, fromCard) {
    walkIndex = Math.max(0, Math.min(data.walk.length - 1, i));
    var buttons = $('cm-steps').querySelectorAll('button');
    for (var k = 0; k < buttons.length; k++) {
      if (k === walkIndex) buttons[k].setAttribute('aria-current', 'step');
      else buttons[k].removeAttribute('aria-current');
    }
    var cur = buttons[walkIndex];
    var dot = $('cm-steps').querySelector('.cm-dot');
    if (cur && dot) dot.style.top = (cur.offsetTop + cur.offsetHeight / 2 - 7) + 'px';
    renderStepPanel();
    if (walkTrace) { render(); renderCard(); placeCard(); }
    if (fromCard) renderCard();
  }

  function renderStepPanel() {
    var step = data.walk[walkIndex];
    var strip = data.walk.map(function (s, i) {
      return '<span class="' + (i === walkIndex ? 'is-on' : i < walkIndex ? 'is-done' : '') + '">' + esc(s.title) + '</span>';
    }).join('<span aria-hidden="true" style="border:0">→</span>');
    $('cm-step-panel').innerHTML =
      '<div class="cm-strip" aria-hidden="true">' + strip + '</div>' +
      '<div class="cm-stitle">' + (walkIndex + 1) + '. ' + esc(step.title) + '</div>' +
      '<p>' + esc(step.text) + '</p>' + miniList(step.crates) +
      '<div class="cm-step-ctl">' +
      '<button type="button" data-act="prev"' + (walkIndex === 0 ? ' disabled' : '') + '>Previous</button>' +
      '<button type="button" data-act="next"' + (walkIndex === data.walk.length - 1 ? ' disabled' : '') + '>Next</button>' +
      '<button type="button" data-act="play" aria-pressed="' + (playTimer ? 'true' : 'false') + '">' + (playTimer ? 'Pause' : 'Play the path') + '</button>' +
      '<button type="button" data-act="trace">Trace on the map</button></div>';
  }

  function startPlay() {
    if (walkIndex === data.walk.length - 1) setWalk(0, false);
    playTimer = setInterval(function () {
      if (walkIndex >= data.walk.length - 1) { stopPlay(); renderStepPanel(); return; }
      setWalk(walkIndex + 1, false);
    }, 3200);
  }
  function stopPlay() { if (playTimer) { clearInterval(playTimer); playTimer = null; } }

  /* ---------- Boot ---------- */

  function fromHash() {
    var m = /^#crate=([a-z0-9-]+)$/.exec(location.hash || '');
    if (m && data.crates[m[1]]) select(m[1], false);
  }

  function init(json) {
    data = json;
    buildTracks();
    buildBands();
    applyFilter();
    buildWalk();
    renderCard();
    $('cm-card').addEventListener('click', onCardClick);
    $('cm-search').addEventListener('input', function (e) { query = e.target.value.trim().toLowerCase(); applyFilter(); });
    $('cm-transitive').addEventListener('change', render);
    document.addEventListener('keydown', function (e) {
      if (e.key === 'Escape' && (selected || walkTrace)) { walkTrace = false; select(null, false); }
    });
    document.querySelectorAll('.cm-hosts a[data-crate]').forEach(function (a) {
      a.addEventListener('click', function (e) {
        e.preventDefault();
        select(a.getAttribute('data-crate'), false);
        $('map').scrollIntoView({ behavior: REDUCED ? 'auto' : 'smooth' });
      });
    });
    var redraw = function () { placeCard(); setWalk(walkIndex, false); };
    if (window.ResizeObserver) new ResizeObserver(redraw).observe($('cm-stage'));
    else window.addEventListener('resize', redraw);
    window.addEventListener('hashchange', fromHash);
    fromHash();
  }

  function boot() {
    fetch('crate-map.json')
      .then(function (r) { if (!r.ok) throw new Error('crate-map.json: HTTP ' + r.status); return r.json(); })
      .then(init)
      .catch(function (err) {
        $('cm-bands').innerHTML = '<p class="cm-empty">The crate map could not load (' + esc(String(err.message || err)) +
          '). The <a href="https://github.com/AndrewAltimit/legend-of-legaia-re#how-it-fits-together">README</a> carries the same map as a picture.</p>';
      });
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', boot);
  else boot();
})();
