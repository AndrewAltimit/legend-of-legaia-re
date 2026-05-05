/* main.js — legend-of-legaia-re site interactions (no dependencies) */

(function () {
  'use strict';

  /* ---------- Mobile sidebar toggle ---------- */
  const toggle = document.querySelector('.sidebar-toggle');
  const sidebar = document.querySelector('.sidebar');

  if (toggle && sidebar) {
    toggle.addEventListener('click', () => {
      const open = sidebar.classList.toggle('open');
      toggle.setAttribute('aria-expanded', String(open));
    });
    sidebar.querySelectorAll('a').forEach(link =>
      link.addEventListener('click', () => {
        sidebar.classList.remove('open');
        toggle.setAttribute('aria-expanded', 'false');
      })
    );
  }

  /* ---------- Active section tracking for sidebar nav ---------- */
  const sidebarLinks = Array.from(document.querySelectorAll('.sidebar-nav a[href^="#"]'));
  if (sidebarLinks.length && 'IntersectionObserver' in window) {
    const map = new Map();
    sidebarLinks.forEach(a => {
      const id = a.getAttribute('href').slice(1);
      const el = id && document.getElementById(id);
      if (el) map.set(el, a);
    });
    const io = new IntersectionObserver(
      entries => {
        entries.forEach(entry => {
          if (entry.isIntersecting) {
            sidebarLinks.forEach(a => a.classList.remove('active'));
            const link = map.get(entry.target);
            if (link) link.classList.add('active');
          }
        });
      },
      { rootMargin: '-20% 0px -70% 0px', threshold: 0 }
    );
    map.forEach((_, el) => io.observe(el));
  }

  /* ---------- Smooth scroll with offset ---------- */
  document.querySelectorAll('a[href^="#"]').forEach(anchor => {
    anchor.addEventListener('click', e => {
      const id = anchor.getAttribute('href');
      if (!id || id === '#') return;
      const target = document.querySelector(id);
      if (!target) return;
      e.preventDefault();
      const top = target.getBoundingClientRect().top + window.scrollY - 16;
      window.scrollTo({ top, behavior: 'smooth' });
      history.replaceState(null, '', id);
    });
  });

  /* ---------- Copy-to-clipboard on code blocks ---------- */
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
        try { document.execCommand('copy'); ok(); } catch { btn.textContent = 'err'; }
        document.body.removeChild(ta);
      }
    });
    pre.appendChild(btn);
  });
})();
