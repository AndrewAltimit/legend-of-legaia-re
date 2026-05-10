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
  function setupCopyButtons() {
    document.querySelectorAll('pre').forEach(pre => {
      if (pre.querySelector('.copy-btn')) return;
      const btn = document.createElement('button');
      btn.className = 'copy-btn';
      btn.type = 'button';
      btn.textContent = 'copy';
      btn.addEventListener('click', () => {
        const code = pre.querySelector('code');
        const text = (code || pre).textContent;
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
      pre.appendChild(btn);
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
