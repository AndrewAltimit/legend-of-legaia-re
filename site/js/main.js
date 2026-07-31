/* main.js - site interactions: scroll-spy, copy buttons, search.
 * Loaded AFTER layout.js, so the DOM injected by injectLayout() is in place.
 */

(function () {
  'use strict';

  /* ---------- TOC rail scroll-spy ---------- */
  function setupTocSpy() {
    const links = Array.from(document.querySelectorAll('.toc-list a'));
    if (!links.length || !('IntersectionObserver' in window)) return;
    const map = new Map();
    links.forEach(a => {
      const id = a.dataset.target;
      const el = id && document.getElementById(id);
      if (el) map.set(el, a);
    });
    if (!map.size) return;
    let active = null;
    const setActive = (a) => {
      if (active === a) return;
      links.forEach(l => l.classList.remove('active'));
      if (a) a.classList.add('active');
      active = a;
    };
    const visible = new Set();
    const io = new IntersectionObserver(entries => {
      entries.forEach(e => {
        if (e.isIntersecting) visible.add(e.target);
        else visible.delete(e.target);
      });
      /* Pick the topmost visible heading */
      const top = Array.from(visible).sort((a, b) =>
        a.getBoundingClientRect().top - b.getBoundingClientRect().top
      )[0];
      if (top) setActive(map.get(top));
    }, { rootMargin: '-15% 0px -75% 0px', threshold: 0 });
    map.forEach((_, el) => io.observe(el));
  }

  /* ---------- Copy-to-clipboard on code blocks ---------- */
  function attachCopyButton(host, getText) {
    if (host.querySelector('.copy-btn')) return;
    const btn = document.createElement('button');
    btn.className = 'copy-btn';
    btn.type = 'button';
    btn.textContent = 'copy';
    btn.addEventListener('click', () => {
      const text = getText();
      const ok = () => {
        btn.textContent = 'copied';
        setTimeout(() => { btn.textContent = 'copy'; }, 1500);
      };
      if (navigator.clipboard?.writeText) {
        navigator.clipboard.writeText(text).then(ok).catch(() => { btn.textContent = 'err'; });
      } else {
        const ta = document.createElement('textarea');
        ta.value = text;
        document.body.appendChild(ta);
        ta.select();
        try { document.execCommand('copy'); ok(); } catch (e) { btn.textContent = 'err'; }
        document.body.removeChild(ta);
      }
    });
    host.appendChild(btn);
  }

  function setupCopyButtons() {
    document.querySelectorAll('pre').forEach(pre => {
      attachCopyButton(pre, () => (pre.querySelector('code') || pre).textContent);
    });
    /* Native-CLI command blocks: copy just the command text. */
    document.querySelectorAll('.cli').forEach(cli => {
      attachCopyButton(cli, () => (cli.querySelector('code') || cli).textContent.trim());
    });
  }

  /* ---------- Search ---------- */
  function setupSearch() {
    const overlay = document.getElementById('search-overlay');
    const input = document.getElementById('search-input');
    const results = document.getElementById('search-results');
    const closeBtn = overlay && overlay.querySelector('.search-close');
    if (!overlay || !input || !results) return;
    const depth = parseInt(overlay.dataset.depth || '0', 10);

    let index = null;
    let activeIdx = 0;
    let currentMatches = [];

    const fetchIndex = () => {
      if (index) return Promise.resolve(index);
      const url = '../'.repeat(depth) + 'search-index.json';
      return fetch(url).then(r => r.ok ? r.json() : []).then(data => {
        index = data || [];
        return index;
      }).catch(() => { index = []; return index; });
    };

    const escapeHtml = s => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    const escapeRe = s => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

    const highlight = (text, query) => {
      if (!query) return escapeHtml(text);
      const re = new RegExp('(' + escapeRe(query) + ')', 'ig');
      return escapeHtml(text).replace(/&lt;.*?&gt;/g, '').replace(re, '<mark>$1</mark>');
    };

    const score = (entry, q) => {
      const ql = q.toLowerCase();
      const t = (entry.title || '').toLowerCase();
      const sec = (entry.section || '').toLowerCase();
      const sn = (entry.snippet || '').toLowerCase();
      let s = 0;
      if (t === ql) s += 100;
      if (t.startsWith(ql)) s += 50;
      if (t.includes(ql)) s += 30;
      if (sec.includes(ql)) s += 20;
      if (sn.includes(ql)) s += 10;
      return s;
    };

    const render = (q) => {
      results.innerHTML = '';
      const all = index || [];
      if (!q) {
        currentMatches = all.slice(0, 30);
      } else {
        currentMatches = all
          .map(e => ({ e, s: score(e, q) }))
          .filter(x => x.s > 0)
          .sort((a, b) => b.s - a.s)
          .slice(0, 30)
          .map(x => x.e);
      }
      if (!currentMatches.length) {
        results.innerHTML = '<li class="search-empty">No matches.</li>';
        return;
      }
      currentMatches.forEach((e, i) => {
        const li = document.createElement('li');
        if (i === 0) li.classList.add('active');
        const hrefBase = e.href || '#';
        const href = '../'.repeat(depth) + hrefBase + (e.anchor ? '#' + e.anchor : '');
        const a = document.createElement('a');
        a.href = href;
        a.innerHTML =
          '<div>' +
            (e.section ? '<span class="res-section">' + escapeHtml(e.section) + '</span>' : '') +
            '<span class="res-title">' + highlight(e.title || '', q) + '</span>' +
          '</div>' +
          (e.snippet ? '<div class="res-snippet">' + highlight(e.snippet, q) + '</div>' : '');
        li.appendChild(a);
        results.appendChild(li);
      });
      activeIdx = 0;
    };

    const setActive = (i) => {
      const items = Array.from(results.querySelectorAll('li'));
      if (!items.length) return;
      i = Math.max(0, Math.min(items.length - 1, i));
      items.forEach(li => li.classList.remove('active'));
      items[i].classList.add('active');
      items[i].scrollIntoView({ block: 'nearest' });
      activeIdx = i;
    };

    const open = () => {
      overlay.classList.add('open');
      input.value = '';
      results.innerHTML = '<li class="search-empty">Loading index…</li>';
      fetchIndex().then(() => {
        render('');
        setTimeout(() => input.focus(), 30);
      });
    };
    const close = () => {
      overlay.classList.remove('open');
    };

    window.openSearch = open;
    window.closeSearch = close;

    if (closeBtn) closeBtn.addEventListener('click', close);

    overlay.addEventListener('click', (e) => {
      if (e.target === overlay) close();
    });

    input.addEventListener('input', () => render(input.value.trim()));

    input.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') { close(); }
      else if (e.key === 'ArrowDown') { e.preventDefault(); setActive(activeIdx + 1); }
      else if (e.key === 'ArrowUp')   { e.preventDefault(); setActive(activeIdx - 1); }
      else if (e.key === 'Enter')     {
        e.preventDefault();
        const items = Array.from(results.querySelectorAll('li a'));
        if (items[activeIdx]) window.location.href = items[activeIdx].href;
      }
    });

    document.addEventListener('keydown', (e) => {
      if (overlay.classList.contains('open') && e.key === 'Escape') close();
    });
  }

  /* ---------- No WebGL2 ---------- */

  /* Every 3D surface on this site - the asset viewer, the minigames, the play
   * page, the world overview, the monster meshes - is WebGL2 only. There has
   * never been a WebGL1 path and building one is not worth it.
   *
   * What was worth fixing is how the absence announced itself. `getContext
   * ('webgl2')` returning null used to surface as a bare `Error('WebGL2 not
   * available')`: the asset viewer caught it and dropped to its flat-shaded
   * loop (so the models came out untextured), the minigames and the play page
   * did not (so the art came out scrambled, or the canvas stayed black behind
   * a one-line status). All three read as "this site is broken" rather than
   * "this browser cannot draw it", which is the most expensive way to be
   * wrong - it sends people hunting through the renderer for a bug that is not
   * there. It has already cost that once: a graphics-driver update left a
   * browser without hardware acceleration until it was restarted, and the
   * pages answered with untextured geometry instead of saying so.
   *
   * So the null context now raises a banner that names the real condition
   * before it throws. Callers keep throwing exactly as before - the viewer's
   * fallback still runs - they just do it through here. */
  var webgl2NoticeShown = false;

  function showWebgl2Notice() {
    if (webgl2NoticeShown) return;
    webgl2NoticeShown = true;
    var host = document.querySelector('.content') || document.body;
    if (!host) return;

    var box = document.createElement('div');
    box.className = 'legaia-webgl2-notice';
    box.setAttribute('role', 'alert');
    /* Sticky, because the canvas that failed is usually well below the fold:
     * a banner pinned to the top of the page scrolls out of sight exactly when
     * the user reaches the blank/untextured area it explains. Offset by the
     * fixed top bar so the two do not overlap, and opaque so the page does not
     * show through it. */
    box.style.cssText = [
      'position:sticky;top:calc(var(--topbar-h,56px) + .5rem);z-index:40',
      'margin:1rem 0;padding:1rem 1.15rem;border-radius:8px',
      'border:1px solid var(--border,#3a4356)',
      'border-left:4px solid #d9a441',
      'background:var(--bg-code,#141a24)',
      'box-shadow:0 6px 18px rgba(0,0,0,.45)',
      'color:var(--text,#dde3ee);font-size:.92rem;line-height:1.55',
    ].join(';');

    var h = document.createElement('strong');
    h.style.cssText = 'display:block;font-size:1rem;margin-bottom:.4rem';
    h.textContent = 'This browser has no WebGL2 - the 3D views cannot draw';
    box.appendChild(h);

    var p = document.createElement('p');
    p.style.cssText = 'margin:.35rem 0';
    p.textContent =
      'Every 3D view here (asset viewer, minigames, the playable port, the '
      + 'world overview) needs WebGL2, and this browser is not providing it. '
      + 'This is a browser or graphics-driver condition, not a fault in the '
      + 'page - nothing on the site is broken, and reloading will not help '
      + 'until the browser can create a WebGL2 context again.';
    box.appendChild(p);

    var lead = document.createElement('p');
    lead.style.cssText = 'margin:.6rem 0 .25rem';
    lead.textContent = 'What to try, most likely first:';
    box.appendChild(lead);

    var ul = document.createElement('ul');
    ul.style.cssText = 'margin:.25rem 0 .25rem 1.1rem;padding:0';
    [
      ['Restart your browser - especially after a graphics-driver update. ',
       'A driver update leaves the already-running browser without hardware '
       + 'acceleration until it is restarted. This is the most common cause '
       + 'and the least obvious one, because everything else keeps working.'],
      ['Firefox: ',
       'check about:support under Graphics for a blocked or failed driver, '
       + 'and try setting webgl.force-enabled to true in about:config.'],
      ['Chrome / Chromium: ',
       'check chrome://gpu, and make sure "Use graphics acceleration when '
       + 'available" is on in Settings > System.'],
    ].forEach(function (pair) {
      var li = document.createElement('li');
      li.style.cssText = 'margin:.2rem 0';
      var b = document.createElement('strong');
      b.textContent = pair[0];
      li.appendChild(b);
      li.appendChild(document.createTextNode(pair[1]));
      ul.appendChild(li);
    });
    box.appendChild(ul);

    host.insertBefore(box, host.firstChild);
    /* The canvas that just failed is usually far down the page, so the banner
     * would sit unread above the fold the user is looking at. */
    try { box.scrollIntoView({ block: 'center' }); } catch (e) {}
  }

  /* Raise the banner and hand back the Error for the caller to throw. */
  window.legaiaWebgl2Failure = function () {
    try { showWebgl2Notice(); } catch (e) { /* never mask the real failure */ }
    var err = new Error('WebGL2 not available');
    err.noWebgl2 = true;
    return err;
  };

  /* ---------- Init ---------- */
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
      setupTocSpy();
      setupCopyButtons();
      setupSearch();
    });
  } else {
    setupTocSpy();
    setupCopyButtons();
    setupSearch();
  }
})();
