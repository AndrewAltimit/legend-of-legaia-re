/* layout.js — shared sidebar nav for the multi-page site.
 *
 * Each page imports this and calls injectLayout({ active: 'subsystems/script-vm' }).
 * The nav structure lives here once; pages don't duplicate it.
 *
 * Pages also get a "you-are-here" breadcrumb generated from the active key.
 */

const NAV = [
  {
    label: 'overview',
    items: [
      { href: 'index.html',                   text: 'Home',              key: 'home' },
      { href: 'architecture.html',            text: 'How it stacks',     key: 'architecture' },
      { href: 'quickstart.html',              text: 'Quick start',       key: 'quickstart' },
    ],
  },
  {
    label: 'subsystems',
    items: [
      { href: 'subsystems/index.html',         text: 'Subsystems index',  key: 'subsystems/index' },
      { href: 'subsystems/boot.html',          text: 'Boot path',         key: 'subsystems/boot' },
      { href: 'subsystems/asset-loader.html',  text: 'Asset loader',      key: 'subsystems/asset-loader' },
      { href: 'subsystems/script-vm.html',     text: 'Field / event VM',  key: 'subsystems/script-vm' },
      { href: 'subsystems/actor-vm.html',      text: 'Actor / sprite VM', key: 'subsystems/actor-vm' },
      { href: 'subsystems/move-vm.html',       text: 'Move-table VM',     key: 'subsystems/move-vm' },
      { href: 'subsystems/effect-vm.html',     text: 'Effect VM',         key: 'subsystems/effect-vm' },
      { href: 'subsystems/battle.html',        text: 'Battle',            key: 'subsystems/battle' },
      { href: 'subsystems/battle-action.html', text: 'Battle action FSM', key: 'subsystems/battle-action' },
      { href: 'subsystems/audio.html',         text: 'Audio',             key: 'subsystems/audio' },
      { href: 'subsystems/renderer.html',      text: 'Renderer',          key: 'subsystems/renderer' },
      { href: 'subsystems/engine.html',        text: 'Engine port plan',  key: 'subsystems/engine' },
    ],
  },
  {
    label: 'formats',
    items: [
      { href: 'formats/index.html',            text: 'Formats index',     key: 'formats/index' },
    ],
  },
  {
    label: 'tooling',
    items: [
      { href: 'tooling/index.html',            text: 'Tooling index',     key: 'tooling/index' },
    ],
  },
  {
    label: 'reference',
    items: [
      { href: 'reference/index.html',          text: 'Reference index',   key: 'reference/index' },
      { href: 'reference/functions.html',      text: 'Key functions',     key: 'reference/functions' },
      { href: 'reference/memory-map.html',     text: 'PSX RAM map',       key: 'reference/memory-map' },
    ],
  },
  {
    label: 'interactive',
    items: [
      { href: 'viewer.html',                   text: 'Asset viewer',      key: 'viewer' },
    ],
  },
];

/* Path resolution helper — pages in subdirs need ../ prefixed. */
function resolveHref(href, depth) {
  if (depth === 0) return href;
  return '../'.repeat(depth) + href;
}

function depthFromKey(key) {
  if (!key || key === 'home') return 0;
  return key.split('/').length - 1;
}

function injectLayout(opts) {
  const { active } = opts || {};
  const depth = depthFromKey(active);

  /* Sidebar nav HTML */
  const sidebar = document.createElement('aside');
  sidebar.className = 'sidebar';
  sidebar.id = 'sidebar';

  const brand = document.createElement('a');
  brand.href = resolveHref('index.html', depth);
  brand.className = 'sidebar-brand';
  brand.innerHTML = '<span class="prompt">$</span>legend-of-legaia-re';
  sidebar.appendChild(brand);

  for (const section of NAV) {
    const sec = document.createElement('div');
    sec.className = 'sidebar-section';
    const lbl = document.createElement('div');
    lbl.className = 'sidebar-section-label';
    lbl.textContent = section.label;
    sec.appendChild(lbl);
    const nav = document.createElement('nav');
    nav.className = 'sidebar-nav';
    nav.setAttribute('aria-label', section.label);
    for (const item of section.items) {
      const a = document.createElement('a');
      a.href = resolveHref(item.href, depth);
      a.textContent = item.text;
      if (item.key === active) a.classList.add('active');
      nav.appendChild(a);
    }
    sec.appendChild(nav);
    sidebar.appendChild(sec);
  }

  const foot = document.createElement('div');
  foot.className = 'sidebar-foot';
  foot.innerHTML =
    '<a href="https://github.com/AndrewAltimit/legend-of-legaia-re" target="_blank" rel="noopener">GitHub →</a><br>' +
    'Tooling: MIT or Apache-2.0.<br>' +
    'No Sony bytes shipped.';
  sidebar.appendChild(foot);

  /* Toggle button (mobile) */
  const toggle = document.createElement('button');
  toggle.className = 'sidebar-toggle';
  toggle.setAttribute('aria-label', 'Toggle navigation');
  toggle.setAttribute('aria-expanded', 'false');
  toggle.innerHTML = '&#9776;';

  toggle.addEventListener('click', () => {
    const open = sidebar.classList.toggle('open');
    toggle.setAttribute('aria-expanded', String(open));
  });

  /* Inject before .app or at body start */
  const app = document.querySelector('.app');
  if (app) {
    app.insertBefore(sidebar, app.firstChild);
    document.body.insertBefore(toggle, document.body.firstChild);
  } else {
    document.body.insertBefore(sidebar, document.body.firstChild);
    document.body.insertBefore(toggle, document.body.firstChild);
  }
}

window.injectLayout = injectLayout;
