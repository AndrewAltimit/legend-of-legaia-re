/* layout.js - shared layout for the multi-page site.
 *
 * Each page calls injectLayout({ active: 'subsystems/script-vm' }).
 * This builds:
 *   - Left sidebar with collapsible sections, active highlight, search trigger.
 *   - In-page TOC rail (auto-built from h2/h3 inside .content).
 *   - Prev/next page footer derived from NAV order.
 *   - Search overlay (filters NAV + headings + page snippets).
 *   - Mobile sidebar toggle and overlay scrim.
 *
 * The structure of NAV below is the single source of truth for nav ordering.
 */

const NAV = [
  {
    label: 'overview',
    items: [
      { href: 'index.html',                     text: 'Home',                     key: 'home' },
      { href: 'architecture.html',              text: 'How it stacks',            key: 'architecture' },
      { href: 'quickstart.html',                text: 'Quick start',              key: 'quickstart' },
    ],
  },
  {
    label: 'explore',
    items: [
      { href: 'viewer.html',                    text: 'Asset viewer',             key: 'viewer' },
      { href: 'audio.html',                     text: 'Audio browser',            key: 'audio' },
      { href: 'world.html',                     text: 'Game world',               key: 'world' },
      { href: 'shops.html',                     text: 'Shops & vendors',          key: 'shops' },
      { href: 'minigames.html',                 text: 'Minigames',                key: 'minigames' },
      { href: 'arts.html',                      text: 'Tactical Arts',            key: 'arts' },
      { href: 'monsters.html',                  text: 'Enemy table',              key: 'monsters' },
      { href: 'world-overview.html',            text: 'World overview (3D)',      key: 'world-overview' },
    ],
  },
  {
    label: 'subsystems',
    items: [
      { href: 'subsystems/index.html',          text: 'Subsystems index',         key: 'subsystems/index' },
      { href: 'subsystems/boot.html',           text: 'Boot path',                key: 'subsystems/boot' },
      { href: 'subsystems/asset-loader.html',   text: 'Asset loader',             key: 'subsystems/asset-loader' },
      // Runtime VMs
      { href: 'subsystems/script-vm.html',      text: 'Field / event VM',         key: 'subsystems/script-vm' },
      { href: 'subsystems/field-locomotion.html', text: 'Field locomotion',       key: 'subsystems/field-locomotion' },
      { href: 'subsystems/actor-vm.html',       text: 'Actor / sprite VM',        key: 'subsystems/actor-vm' },
      { href: 'subsystems/move-vm.html',        text: 'Move-table VM',            key: 'subsystems/move-vm' },
      { href: 'subsystems/motion-vm.html',      text: 'Motion VM',                key: 'subsystems/motion-vm' },
      { href: 'subsystems/effect-vm.html',      text: 'Effect VM',                key: 'subsystems/effect-vm' },
      // Battle
      { href: 'subsystems/battle.html',         text: 'Battle',                   key: 'subsystems/battle' },
      { href: 'subsystems/battle-action.html',  text: 'Battle action FSM',        key: 'subsystems/battle-action' },
      { href: 'subsystems/battle-formulas.html',text: 'Battle formulas',          key: 'subsystems/battle-formulas' },
      // Per-domain runtime
      { href: 'subsystems/world-map.html',      text: 'World map',                key: 'subsystems/world-map' },
      { href: 'subsystems/world-overview-viewer.html', text: 'World-overview viewer', key: 'subsystems/world-overview-viewer' },
      { href: 'subsystems/save-screen.html',    text: 'Save screen',              key: 'subsystems/save-screen' },
      { href: 'subsystems/shop.html',           text: 'Shop',                     key: 'subsystems/shop' },
      { href: 'subsystems/inn.html',            text: 'Inn',                      key: 'subsystems/inn' },
      { href: 'subsystems/level-up.html',       text: 'Level-up',                 key: 'subsystems/level-up' },
      { href: 'subsystems/cutscene.html',       text: 'Cutscene (STR)',           key: 'subsystems/cutscene' },
      // Output
      { href: 'subsystems/audio.html',          text: 'Audio',                    key: 'subsystems/audio' },
      { href: 'subsystems/renderer.html',       text: 'Renderer',                 key: 'subsystems/renderer' },
      { href: 'subsystems/engine.html',         text: 'Engine port plan',         key: 'subsystems/engine' },
    ],
  },
  {
    label: 'formats',
    items: [
      { href: 'formats/index.html',                  text: 'Formats index',            key: 'formats/index' },
      // Disc + container layer
      { href: 'formats/disc.html',                   text: 'PSX disc geometry',        key: 'formats/disc' },
      { href: 'formats/prot.html',                   text: 'PROT.DAT TOC',             key: 'formats/prot' },
      { href: 'formats/cdname.html',                 text: 'CDNAME.TXT name map',      key: 'formats/cdname' },
      { href: 'formats/dmy.html',                    text: 'DMY.DAT (dev fixtures)',   key: 'formats/dmy' },
      // Compression + dispatch
      { href: 'formats/lzs.html',                    text: 'Legaia LZS',               key: 'formats/lzs' },
      { href: 'formats/asset-type.html',             text: 'Asset type dispatcher',    key: 'formats/asset-type' },
      { href: 'formats/asset-descriptor.html',       text: 'Asset descriptor',         key: 'formats/asset-descriptor' },
      { href: 'formats/data-field.html',             text: 'DATA_FIELD streaming',     key: 'formats/data-field' },
      { href: 'formats/pack.html',                   text: 'Pack format',              key: 'formats/pack' },
      { href: 'formats/tim-pack.html',               text: 'TIM-pack',                 key: 'formats/tim-pack' },
      { href: 'formats/field-pack.html',             text: 'Field-pack',               key: 'formats/field-pack' },
      { href: 'formats/battle-data-pack.html',       text: 'Battle-data pack',         key: 'formats/battle-data-pack' },
      { href: 'formats/effect.html',                 text: 'Effect bundles',           key: 'formats/effect' },
      { href: 'formats/scene-bundles.html',          text: 'Scene bundles',            key: 'formats/scene-bundles' },
      // Per-asset
      { href: 'formats/tim.html',                    text: 'PSX TIM',                  key: 'formats/tim' },
      { href: 'formats/tmd.html',                    text: 'Legaia TMD',               key: 'formats/tmd' },
      { href: 'formats/vab.html',                    text: 'VAB sound bank',           key: 'formats/vab' },
      { href: 'formats/seq.html',                    text: 'PsyQ SEQ',                 key: 'formats/seq' },
      { href: 'formats/xa.html',                     text: 'XA-ADPCM',                 key: 'formats/xa' },
      { href: 'formats/mes.html',                    text: 'MES dialog',               key: 'formats/mes' },
      { href: 'formats/anm.html',                    text: 'ANM animation',            key: 'formats/anm' },
      { href: 'formats/mdt.html',                    text: 'MDT move table',           key: 'formats/mdt' },
      { href: 'formats/art-data.html',               text: 'Art data',                 key: 'formats/art-data' },
      { href: 'formats/dialog-font.html',            text: 'Dialog font',              key: 'formats/dialog-font' },
      // Auxiliary
      { href: 'formats/sound-driver.html',           text: 'Sound-driver paths',       key: 'formats/sound-driver' },
      { href: 'formats/pochi.html',                  text: 'Pochi-filler',             key: 'formats/pochi' },
      { href: 'formats/mips-overlay.html',           text: 'MIPS overlay code',        key: 'formats/mips-overlay' },
      { href: 'formats/overlay-ptr-table.html',      text: 'Overlay ptr-table code',   key: 'formats/overlay-ptr-table' },
      { href: 'formats/navmesh.html',                text: 'Per-scene scratch buffer', key: 'formats/navmesh' },
      { href: 'formats/encounter.html',              text: 'Encounter record',         key: 'formats/encounter' },
      { href: 'formats/str-fmv-table.html',          text: 'STR FMV table',            key: 'formats/str-fmv-table' },
      { href: 'formats/save-record.html',            text: 'Per-character save record', key: 'formats/save-record' },
    ],
  },
  {
    label: 'tooling',
    items: [
      { href: 'tooling/index.html',                  text: 'Tooling index',            key: 'tooling/index' },
      { href: 'tooling/extraction.html',             text: 'Extraction CLIs',          key: 'tooling/extraction' },
      { href: 'tooling/ghidra.html',                 text: 'Ghidra in Docker',         key: 'tooling/ghidra' },
      { href: 'tooling/overlay-capture.html',        text: 'Overlay capture',          key: 'tooling/overlay-capture' },
      { href: 'tooling/mednafen-automation.html',    text: 'Mednafen automation',      key: 'tooling/mednafen-automation' },
      { href: 'tooling/pcsx-redux-automation.html',  text: 'PCSX-Redux automation',    key: 'tooling/pcsx-redux-automation' },
    ],
  },
  {
    label: 'reference',
    items: [
      { href: 'reference/index.html',           text: 'Reference index',          key: 'reference/index' },
      { href: 'reference/functions.html',       text: 'Key functions',            key: 'reference/functions' },
      { href: 'reference/memory-map.html',      text: 'PSX RAM map',              key: 'reference/memory-map' },
      { href: 'reference/cheats.html',          text: 'Cheat databases',          key: 'reference/cheats' },
      { href: 'reference/gamedata.html',        text: 'Curated game-data tables', key: 'reference/gamedata' },
    ],
  },
];

/* ---------- Helpers ---------- */
function resolveHref(href, depth) {
  if (depth === 0) return href;
  if (/^https?:/.test(href)) return href;
  return '../'.repeat(depth) + href;
}

function depthFromKey(key) {
  if (!key || key === 'home') return 0;
  return key.split('/').length - 1;
}

function flattenNav() {
  const out = [];
  for (const section of NAV) for (const item of section.items) out.push(item);
  return out;
}

function findSiblings(activeKey) {
  const flat = flattenNav();
  const idx = flat.findIndex(x => x.key === activeKey);
  if (idx < 0) return { prev: null, next: null };
  return {
    prev: idx > 0 ? flat[idx - 1] : null,
    next: idx < flat.length - 1 ? flat[idx + 1] : null,
  };
}

function slugify(s) {
  return s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '');
}

/* ---------- Sidebar ---------- */
function buildSidebar(active, depth) {
  const sidebar = document.createElement('aside');
  sidebar.className = 'sidebar';
  sidebar.id = 'sidebar';

  const brand = document.createElement('a');
  brand.href = resolveHref('index.html', depth);
  brand.className = 'sidebar-brand';
  brand.innerHTML = '<span class="prompt">$</span>legend-of-legaia-re';
  sidebar.appendChild(brand);

  /* Search trigger button */
  const searchBtn = document.createElement('button');
  searchBtn.type = 'button';
  searchBtn.className = 'sidebar-search';
  searchBtn.id = 'open-search';
  searchBtn.setAttribute('aria-label', 'Open search');
  searchBtn.innerHTML =
    '<span class="icon">⌕</span>' +
    '<span class="label">Search the site</span>' +
    '<span class="kbd">/</span>';
  sidebar.appendChild(searchBtn);

  for (const section of NAV) {
    const sec = document.createElement('div');
    sec.className = 'sidebar-section';
    sec.dataset.section = section.label;

    const hasActive = section.items.some(item => item.key === active);
    if (hasActive) sec.classList.add('has-active');

    /* Section header (toggle) */
    const tog = document.createElement('button');
    tog.type = 'button';
    tog.className = 'sidebar-section-toggle';
    tog.innerHTML = '<span class="arrow">▾</span>' + section.label;
    tog.addEventListener('click', () => {
      sec.classList.toggle('collapsed');
      try {
        const persisted = JSON.parse(localStorage.getItem('sidebar-collapsed') || '{}');
        persisted[section.label] = sec.classList.contains('collapsed');
        localStorage.setItem('sidebar-collapsed', JSON.stringify(persisted));
      } catch (e) {}
    });
    sec.appendChild(tog);

    /* Item list */
    const nav = document.createElement('nav');
    nav.className = 'sidebar-nav';
    nav.setAttribute('aria-label', section.label);
    for (const item of section.items) {
      const a = document.createElement('a');
      a.href = resolveHref(item.href, depth);
      a.textContent = item.text;
      a.dataset.key = item.key;
      if (item.key === active) a.classList.add('active');
      nav.appendChild(a);
    }
    sec.appendChild(nav);

    /* Restore collapsed state from localStorage (don't collapse the active section) */
    try {
      const persisted = JSON.parse(localStorage.getItem('sidebar-collapsed') || '{}');
      if (persisted[section.label] && !hasActive) sec.classList.add('collapsed');
    } catch (e) {}

    sidebar.appendChild(sec);
  }

  const foot = document.createElement('div');
  foot.className = 'sidebar-foot';
  foot.innerHTML =
    '<a href="https://github.com/AndrewAltimit/legend-of-legaia-re" target="_blank" rel="noopener">GitHub →</a><br>' +
    'Tooling: MIT or Unlicense.<br>' +
    'No Sony bytes shipped.';
  sidebar.appendChild(foot);

  return sidebar;
}

/* ---------- Heading ID assignment (before anchors / TOC) ---------- */
function assignHeadingIds() {
  const content = document.querySelector('.content');
  if (!content) return;
  content.querySelectorAll('section.doc-section h2, section.doc-section h3, section.doc-section h4').forEach(h => {
    if (h.id) return;
    const sec = h.closest('section.doc-section');
    if (h.tagName === 'H2' && sec && sec.id) {
      h.id = sec.id;
    } else {
      h.id = slugify(h.textContent || '') || ('h-' + Math.random().toString(36).slice(2, 8));
    }
  });
}

/* ---------- TOC rail ---------- */
function buildTocRail() {
  const content = document.querySelector('.content');
  if (!content) return null;

  /* Only consider h2 and h3 inside doc-section (not the page-header h1). */
  const headings = content.querySelectorAll('section.doc-section h2, section.doc-section h3');
  if (headings.length < 2) return null;

  const rail = document.createElement('aside');
  rail.className = 'toc-rail';
  rail.setAttribute('aria-label', 'On this page');

  const title = document.createElement('div');
  title.className = 'toc-title';
  title.textContent = 'On this page';
  rail.appendChild(title);

  const list = document.createElement('ul');
  list.className = 'toc-list';

  headings.forEach(h => {
    const li = document.createElement('li');
    const a = document.createElement('a');
    a.href = '#' + h.id;
    a.textContent = (h.textContent || '').trim();
    a.dataset.target = h.id;
    if (h.tagName === 'H3') a.classList.add('h3');
    li.appendChild(a);
    list.appendChild(li);
  });

  rail.appendChild(list);
  return rail;
}

/* ---------- Heading anchor links (clickable § on h2/h3/h4) ---------- */
/* Call AFTER assignHeadingIds() and AFTER buildTocRail() so the § isn't
   captured into TOC link text. */
function injectHeadingAnchors() {
  const content = document.querySelector('.content');
  if (!content) return;
  content.querySelectorAll('section.doc-section h2, section.doc-section h3, section.doc-section h4').forEach(h => {
    if (h.querySelector('.h-anchor') || !h.id) return;
    const a = document.createElement('a');
    a.className = 'h-anchor';
    a.href = '#' + h.id;
    a.setAttribute('aria-label', 'Anchor link');
    a.textContent = '§';
    h.appendChild(a);
  });
}

/* ---------- Prev/next footer ---------- */
function buildPageNav(active, depth) {
  const { prev, next } = findSiblings(active);
  if (!prev && !next) return null;

  const nav = document.createElement('nav');
  nav.className = 'page-nav';
  nav.setAttribute('aria-label', 'Previous and next page');

  if (prev) {
    const a = document.createElement('a');
    a.href = resolveHref(prev.href, depth);
    a.className = 'pn-prev';
    a.innerHTML =
      '<div class="pn-label">Previous</div>' +
      '<div class="pn-title">' + prev.text + '</div>';
    nav.appendChild(a);
  }
  if (next) {
    const a = document.createElement('a');
    a.href = resolveHref(next.href, depth);
    a.className = 'pn-next';
    a.innerHTML =
      '<div class="pn-label">Next</div>' +
      '<div class="pn-title">' + next.text + '</div>';
    nav.appendChild(a);
  }
  return nav;
}

/* ---------- Mobile toggle button ---------- */
function buildMobileToggle() {
  const toggle = document.createElement('button');
  toggle.className = 'sidebar-toggle';
  toggle.setAttribute('aria-label', 'Toggle navigation');
  toggle.setAttribute('aria-expanded', 'false');
  toggle.innerHTML = '&#9776;';
  return toggle;
}

/* ---------- Search overlay ---------- */
function buildSearchOverlay(depth) {
  const overlay = document.createElement('div');
  overlay.className = 'search-overlay';
  overlay.id = 'search-overlay';
  overlay.innerHTML = `
    <div class="search-box" role="dialog" aria-label="Search">
      <div class="search-input-wrap">
        <span class="icon">⌕</span>
        <input type="text" class="search-input" id="search-input" placeholder="Search pages, sections, formats, functions..." aria-label="Search query">
        <button type="button" class="search-close" aria-label="Close">esc</button>
      </div>
      <ul class="search-results" id="search-results" role="listbox"></ul>
      <div class="search-foot">
        <span><kbd>↑</kbd><kbd>↓</kbd> navigate</span>
        <span><kbd>↵</kbd> open</span>
        <span><kbd>esc</kbd> close</span>
      </div>
    </div>
  `;
  overlay.dataset.depth = String(depth);
  return overlay;
}

/* ---------- Main ---------- */
function injectLayout(opts) {
  const { active } = opts || {};
  const depth = depthFromKey(active);

  const sidebar = buildSidebar(active, depth);
  const toggle = buildMobileToggle();
  const overlay = buildSearchOverlay(depth);

  /* Scrim for mobile sidebar */
  const scrim = document.createElement('div');
  scrim.className = 'sidebar-overlay';
  scrim.id = 'sidebar-scrim';

  toggle.addEventListener('click', () => {
    const open = sidebar.classList.toggle('open');
    toggle.setAttribute('aria-expanded', String(open));
    scrim.classList.toggle('show', open);
  });
  scrim.addEventListener('click', () => {
    sidebar.classList.remove('open');
    toggle.setAttribute('aria-expanded', 'false');
    scrim.classList.remove('show');
  });

  /* Inject sidebar + scrim + toggle */
  const app = document.querySelector('.app');
  if (app) {
    app.insertBefore(sidebar, app.firstChild);
  } else {
    document.body.insertBefore(sidebar, document.body.firstChild);
  }
  document.body.insertBefore(toggle, document.body.firstChild);
  document.body.appendChild(scrim);
  document.body.appendChild(overlay);

  /* Order matters: assign IDs first → build TOC (clean text) → add § anchors */
  assignHeadingIds();
  const toc = buildTocRail();
  injectHeadingAnchors();
  if (toc && app) {
    app.appendChild(toc);
  } else if (app) {
    app.classList.add('no-toc');
  }

  /* Prev/next inside content */
  const content = document.querySelector('.content');
  if (content) {
    const pn = buildPageNav(active, depth);
    if (pn) content.appendChild(pn);
  }

  /* Wire search trigger */
  const openSearch = document.getElementById('open-search');
  if (openSearch) openSearch.addEventListener('click', () => window.openSearch && window.openSearch());

  /* Global keyboard shortcut for search */
  document.addEventListener('keydown', (e) => {
    if (e.target && (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA' || e.target.isContentEditable)) return;
    if (e.key === '/') {
      e.preventDefault();
      window.openSearch && window.openSearch();
    }
  });
}

window.injectLayout = injectLayout;
window.SITE_NAV = NAV;
