/* Expansion workbench: the frame around the editors.
 *
 * The page opens on the map builder, Sims-style: a strip of the pack's maps
 * to pick from, a mode toolbar (Build, Paint terrain, Place objects, NPCs,
 * Zones & exits, Test-play) over the 3D view, and a catalog strip under it.
 * Every other section editor sits behind one of five areas (Build, Story,
 * Battle, Items & cards, Modes & music) plus Pack settings, each area a row
 * of plain-named pages. This module draws that frame; the editors stay in
 * their own files and the app (expansion-workbench-app.js) owns the state.
 *
 * The build tools seam. The mode toolbar never reaches into the painter or
 * the 3D editor itself: a click calls the registered build tools'
 * `setMode(mode, env)` and asks them to fill the catalog strip
 * (`catalog(mode, el, env)`, true when they filled it). A fallback set,
 * registered here, drives the painter tool (`setPainterTool`, map.js), the
 * 3D edit mode (`setEditMode`, gizmos.js) and the preview's camera
 * (`preview.setCamera`); the build-mode lane replaces it with
 * `registerBuildTools(impl)`, and calls `syncBuildMode(mode)` when its own
 * keys change the mode so the toolbar follows. `env` is
 * `{ map, preview, pack, rerender, jump, playHere, playPack }`.
 */
import { h } from './expansion-workbench-forms.js';
import { setPainterTool } from './expansion-workbench-map.js';
import { setEditMode } from './expansion-workbench-gizmos.js';

/** The areas of the top navigation, each a row of pages (app TABS ids). */
export const AREAS = [
  { id: 'build', label: 'Build', title: 'Lay out maps in 3D: ground, objects, people, zones', tabs: ['maps'] },
  { id: 'story', label: 'Story', title: 'Quests, dialogue, cutscenes and the story map', tabs: ['quests', 'dialog', 'cutscenes', 'flow'] },
  { id: 'battle', label: 'Battle', title: 'Monsters, fights, the level curve, arts and Seru spells', tabs: ['battle', 'progression', 'arts', 'spells'] },
  { id: 'stuff', label: 'Items & cards', title: 'Items, equipment, shops and trading cards', tabs: ['items', 'cards'] },
  { id: 'fun', label: 'Modes & music', title: 'Custom game modes, music and sound', tabs: ['modes', 'audio'] },
  { id: 'pack', label: 'Pack settings', title: 'The pack\'s name, id, credits and its files', tabs: ['manifest', 'files'], aside: true },
];

/** Page labels: plain words, the file each edits beside it. */
export const PAGE_LABELS = {
  maps: 'Maps', quests: 'Quests', dialog: 'Dialogue', cutscenes: 'Cutscenes', flow: 'Story map',
  battle: 'Monsters & fights', progression: 'Level curve', arts: 'Arts', spells: 'Seru spells',
  items: 'Items & shops', cards: 'Trading cards', modes: 'Game modes', audio: 'Music & sound',
  manifest: 'Pack details', files: 'Pack files',
};

/** What each page is for, shown under the page row. */
const PAGE_LEADS = {
  quests: 'Goals for the player: what to do, what it unlocks, what it pays.',
  dialog: 'Every line a character says. Lines are used by NPCs, triggers, quests and cutscenes.',
  cutscenes: 'Scripted scenes: walking, camera moves, fades and talk, played on a map.',
  flow: 'The whole story at a glance: how maps, quests, bosses and flags lead into each other.',
  battle: 'New monsters built on retail ones, the groups they fight in, and bosses.',
  progression: 'How fast the party levels up, and how hard enemies get.',
  arts: 'New Tactical Arts: a button combo and the moves it strings together.',
  spells: 'New Seru magic, cast like a retail Seru with the pack\'s numbers.',
  items: 'Consumables, equipment and sets, and the shops that sell them.',
  cards: 'Trading cards, booster packs, the starter deck and card dealers.',
  modes: 'Custom game modes on your maps: poker, card duels, golf, races, prop hunt.',
  audio: 'Your own music and sound effects, and which music plays where.',
  manifest: 'The pack\'s name, id, version, author and what it needs to run.',
  files: 'The pack as the files it exports to.',
};

export const areaOf = (tab) => (AREAS.find((a) => a.tabs.includes(tab)) || AREAS[0]);

/** The mode toolbar. `key` is the digit that picks the mode on the Build area. */
export const BUILD_MODES = [
  { id: 'build', label: 'Build', key: '1', title: 'Pick up, move, turn and scale anything on the map' },
  { id: 'terrain', label: 'Paint terrain', key: '2', title: 'Paint floor and walls tile by tile, or sculpt the ground' },
  { id: 'objects', label: 'Place objects', key: '3', title: 'Put furniture, plants and props on the map' },
  { id: 'npcs', label: 'NPCs', key: '4', title: 'Place people and give them lines' },
  { id: 'zones', label: 'Zones & exits', key: '5', title: 'Doors out, places to start, step-on events and monster zones' },
  { id: 'test', label: 'Test-play', key: '6', title: 'Walk around the map as the player' },
];

// 24 x 24 stroke icons, one per mode.
const ICONS = {
  build: 'M5 3l12 8-5 1.5L9.5 18z M13 13l5 5',
  terrain: 'M3 19l5-8 4 5 3-4 6 7z M15 5a2 2 0 1 0 0.01 0',
  objects: 'M4 10h16 M6 10v9 M18 10v9 M5 6h14l1 4H4z',
  npcs: 'M12 4a3 3 0 1 0 0.01 0 M6 20c0-4 3-6 6-6s6 2 6 6',
  zones: 'M4 4h7v7H4z M14 13h6v7h-6z M14 4l6 6 M11 17H4',
  test: 'M8 5l11 7-11 7z',
};

function icon(id) {
  const ns = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(ns, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('class', 'xw-mode-ico');
  const p = document.createElementNS(ns, 'path');
  p.setAttribute('d', ICONS[id] || '');
  svg.append(p);
  return svg;
}

// --- The build tools seam ------------------------------------------------------------
const B = { mode: 'build', impl: null, bar: null, catalog: null, env: null };

/** The fallback build tools over the painter, the 3D editor and the preview. */
const FALLBACK = {
  setMode(mode, env) {
    const map = env.map ? env.pack.maps[env.map] : null;
    const terrain = !!(map && map.terrain);
    const painter = { build: 'select', terrain: 'paint', objects: 'prop', npcs: 'npc', zones: 'trigger', test: null }[mode];
    const edit = { build: 'select', terrain: terrain ? 'sculpt' : 'view', objects: 'select', npcs: 'select', zones: 'select', test: 'view' }[mode];
    if (painter) setPainterTool(painter);
    setEditMode(edit);
    const pv = env.preview;
    if (pv && pv.setCamera) pv.setCamera(mode === 'test' ? 'player' : 'orbit');
    env.rerender();
  },
};

/** The build-mode lane's tools: `{ setMode(mode, env), catalog?(mode, el, env) }`. */
export function registerBuildTools(impl) {
  B.impl = impl || null;
  drawBar();
  drawCatalog();
}

/** The mode the toolbar shows. */
export const buildMode = () => B.mode;

/** The tools changed the mode themselves (a key): the toolbar follows. */
export function syncBuildMode(mode) {
  if (!BUILD_MODES.some((m) => m.id === mode) || mode === B.mode) return;
  B.mode = mode;
  drawBar();
  drawCatalog();
}

/** Pick mode `mode` (a toolbar click, its digit key). */
export function setBuildMode(mode) {
  if (!BUILD_MODES.some((m) => m.id === mode) || !B.env) return;
  B.mode = mode;
  const tools = B.impl || FALLBACK;
  try { tools.setMode(mode, B.env()); } catch (e) { console.error(e); }
  drawBar();
  drawCatalog();
}

function drawBar() {
  const bar = B.bar;
  if (!bar) return;
  bar.innerHTML = '';
  for (const m of BUILD_MODES) {
    bar.append(h('button', {
      type: 'button', class: 'xw-mode' + (B.mode === m.id ? ' is-on' : ''), 'aria-pressed': String(B.mode === m.id),
      title: `${m.title} (${m.key})`, 'data-mode': m.id, onclick: () => setBuildMode(m.id),
    }, icon(m.id), h('span', { class: 'xw-mode-label' }, m.label)));
  }
}

const chip = (label, title, fn, on) => h('button', { type: 'button', class: 'xw-chip' + (on ? ' is-on' : ''), title, onclick: fn }, label);

// The fallback catalog: the mode's tools and the map's own objects of the kind.
const CATALOG_LISTS = {
  objects: [['props', 'Objects on this map', (o) => o.id || o.label || 'object']],
  npcs: [['npcs', 'People on this map', (o) => o.label || o.name || o.id || 'NPC']],
  zones: [
    ['exits', 'Exits', (o) => o.id || (o.to && (o.to.map || o.to.scene && o.to.scene.name)) || 'exit'],
    ['spawns', 'Start points', (o) => o.id || 'spawn'],
    ['triggers', 'Step-on events', (o) => o.id || 'trigger'],
    ['encounters', 'Monster zones', (o) => o.id || (o.formation || 'encounter')],
  ],
};

const CATALOG_HINTS = {
  build: 'Click anything in the 3D view or on the tile map to pick it up. Drag to move it; W, E and R switch between move, turn and scale; Delete removes it; Ctrl+Z takes any change back.',
  terrain: 'Choose a ground type in the palette beside the tile map, then paint. On a map with sculpted terrain the 3D view raises, lowers and smooths the ground with a brush ([ and ] size it).',
  objects: 'Choose an object from the base scene in the tile map\'s side panel, then click a tile to place it. Click a placed one below to select it.',
  npcs: 'Click a tile to place a person, then pick who they are and what they say in the form that opens. Click one below to select it.',
  zones: 'Exits lead to another map or a retail scene; start points are where the player arrives; step-on events show a line, a notice or start a fight; monster zones roll random battles.',
  test: 'Walk the map as the player, in the 3D view or in a play tab of its own. Edits made while a play tab is open reload there.',
};

function drawCatalog() {
  const el = B.catalog;
  if (!el || !B.env) return;
  el.innerHTML = '';
  const env = B.env();
  if (B.impl && B.impl.catalog) {
    try { if (B.impl.catalog(B.mode, el, env)) return; } catch (e) { console.error(e); }
    el.innerHTML = '';
  }
  const mode = BUILD_MODES.find((m) => m.id === B.mode);
  const map = env.map && env.pack && env.pack.maps ? env.pack.maps[env.map] : null;
  el.append(h('div', { class: 'xw-cat-head' }, h('strong', {}, mode.label), h('span', { class: 'xw-hint' }, ' ' + CATALOG_HINTS[mode.id])));
  const row = h('div', { class: 'xw-cat-row' });
  if (mode.id === 'terrain') {
    row.append(chip('Paint tiles', 'Paint the chosen ground type (B)', () => { setPainterTool('paint'); env.rerender(); }),
      chip('Fill a rectangle', 'Drag a rectangle of the chosen ground (R)', () => { setPainterTool('rect'); env.rerender(); }),
      chip('Pick a ground type', 'Click a tile to take its ground type (I)', () => { setPainterTool('pick'); env.rerender(); }));
    if (map && map.terrain) row.append(chip('Sculpt the ground', 'Raise, lower and smooth the 3D ground (K)', () => { setPainterTool('sculpt'); setEditMode('sculpt'); env.rerender(); }));
  } else if (mode.id === 'zones') {
    row.append(chip('Add an exit', 'Drag a rectangle where the player leaves the map (E)', () => { setPainterTool('exit'); env.rerender(); }),
      chip('Add a start point', 'Click where the player arrives (S)', () => { setPainterTool('spawn'); env.rerender(); }),
      chip('Add a step-on event', 'Drag a rectangle that shows a line or starts a fight (T)', () => { setPainterTool('trigger'); env.rerender(); }),
      chip('Add a monster zone', 'Drag a rectangle where random battles roll (N)', () => { setPainterTool('encounter'); env.rerender(); }));
  } else if (mode.id === 'test') {
    if (env.preview && env.preview.setCamera) row.append(chip('Walk here in 3D', 'Take control of the player in the 3D view; Esc gives the camera back', () => env.preview.setCamera('player')));
    row.append(chip('Play this map in a new tab', 'Open a play tab on this map (it reloads in place on later edits)', () => env.playHere()),
      chip('Play the whole pack', 'Install the pack in the play page and start it', () => env.playPack()));
  } else if (mode.id === 'build') {
    row.append(chip('Fit the view', 'Frame the whole map', () => { if (env.preview && env.preview.setCamera) env.preview.setCamera('orbit'); }));
  }
  if (row.firstChild) el.append(row);
  for (const [list, title, name] of CATALOG_LISTS[mode.id] || []) {
    const items = (map && map[list]) || [];
    const box = h('div', { class: 'xw-cat-list' }, h('span', { class: 'xw-cat-title' }, `${title} (${items.length})`));
    if (!items.length) box.append(h('span', { class: 'xw-hint' }, ' none yet'));
    items.slice(0, 60).forEach((o, i) => box.append(h('button', {
      type: 'button', class: 'xw-cat-item', title: 'Select it and open its form',
      onclick: () => env.jump({ path: `maps/${env.map}.toml`, field: `${list}[${i}]` }),
    }, String(name(o)))));
    if (items.length > 60) box.append(h('span', { class: 'xw-hint' }, ` and ${items.length - 60} more: use Find`));
    el.append(box);
  }
}

// --- The map picker --------------------------------------------------------------------
function mapSize(m) {
  const rows = (m.tiles && m.tiles.rows) || [];
  return [rows.reduce((n, r) => Math.max(n, [...r].length), 0), rows.length];
}

/** A small room over the disc's first scene: what "New map" adds. */
export function blankMap(cat) {
  const scene = cat && cat.scenes && cat.scenes[0] ? cat.scenes[0].name : 'town01';
  return {
    name: 'New map', size: null,
    base: { scene: { kind: 'scene', name: scene }, geometry: 'none' },
    floor_heights: [0, 64],
    tiles: {
      origin: [16, 16],
      rows: ['###########', '#.........#', '#.........#', '#.........#', '#.........#', '#.........#', '###########'],
      legend: { '#': { label: 'wall', walls: 'all', height: 0, ground: null }, '.': { label: 'floor', height: 0, ground: null } },
    },
    props: [], npcs: [], spawns: [{ id: 'start', tile: [5, 3], facing: 0 }], exits: [], triggers: [], encounters: [],
    bgm: null, camera: 'default',
  };
}

/**
 * Draw the Build area's head into `el`: the map picker and the mode
 * toolbar. `o`: { pack, cat, open (map id), choose(id), add(), errorsOf(id),
 * catalog (the strip element), env() }.
 */
export function renderBuildHead(el, o) {
  el.innerHTML = '';
  B.env = o.env;
  B.catalog = o.catalog;
  const maps = (o.pack && o.pack.maps) || {};
  const ids = Object.keys(maps);
  const pick = h('div', { class: 'xw-maps', role: 'listbox', 'aria-label': 'Your maps' });
  for (const id of ids) {
    const m = maps[id];
    const [w, ht] = mapSize(m);
    const errs = o.errorsOf(id);
    const scene = m.base && m.base.scene && m.base.scene.name;
    pick.append(h('button', {
      type: 'button', role: 'option', class: 'xw-map-card' + (id === o.open ? ' is-on' : '') + (errs ? ' has-err' : ''),
      'aria-selected': String(id === o.open), title: `Open ${m.name || id} (maps/${id}.toml)`, onclick: () => o.choose(id),
    }, h('strong', {}, m.name || id),
    h('span', { class: 'xw-map-meta' }, `${w} x ${ht} tiles${scene ? ' - on ' + scene : ''}`),
    h('span', { class: 'xw-map-meta' }, `${(m.npcs || []).length} people, ${(m.props || []).length} objects, ${(m.exits || []).length} exits`),
    errs ? h('span', { class: 'xw-badge' }, errs) : null));
  }
  pick.append(h('button', { type: 'button', class: 'xw-map-card is-add', title: 'Add a small walled room to build on', onclick: () => o.add() },
    h('strong', {}, '+ New map'), h('span', { class: 'xw-map-meta' }, 'a small room to start from')));
  el.append(h('div', { class: 'xw-maps-head' }, h('span', { class: 'wb-label' }, 'Your maps'),
    h('span', { class: 'xw-hint' }, ids.length ? 'pick one to build on' : 'none yet: add one to start building')), pick);
  if (!ids.length) {
    el.append(h('div', { class: 'xw-emptybig' }, h('strong', {}, 'This pack has no maps yet.'),
      h('span', {}, 'A map is a place the player can walk around: a room, a village, a cave. It is laid out tile by tile over a scene from your disc.'),
      h('button', { type: 'button', class: 'wb-button', onclick: () => o.add() }, 'Make your first map')));
    B.bar = null;
    if (B.catalog) B.catalog.innerHTML = '';
    return;
  }
  B.bar = h('div', { class: 'xw-modebar', role: 'toolbar', 'aria-label': 'Build modes' });
  el.append(B.bar);
  drawBar();
  drawCatalog();
}

/**
 * The top navigation: the area row and the open area's page row.
 * `o`: { tab, counts(tab) -> [errors, warnings], go(tab) }.
 */
export function renderNav(nav, o) {
  nav.innerHTML = '';
  const cur = areaOf(o.tab);
  const badge = (e, w) => (e ? h('span', { class: 'xw-badge' }, e) : w ? h('span', { class: 'xw-badge is-warn' }, w) : null);
  const row = h('div', { class: 'xw-areas', role: 'tablist', 'aria-label': 'Workbench areas' });
  AREAS.forEach((a, i) => {
    let e = 0, w = 0;
    for (const t of a.tabs) { const [x, y] = o.counts(t); e += x; w += y; }
    const on = a === cur;
    row.append(h('button', {
      type: 'button', role: 'tab', class: 'xw-area' + (on ? ' is-active' : '') + (a.aside ? ' is-aside' : ''), 'aria-selected': String(on),
      title: `${a.title} (Alt+${i + 1})`, onclick: () => o.go(on ? o.tab : (a.last || a.tabs[0])),
    }, a.aside ? h('span', { class: 'xw-area-gear', 'aria-hidden': 'true' }, '⚙ ') : null, a.label, badge(e, w)));
  });
  nav.append(row);
  cur.last = o.tab;
  if (cur.tabs.length > 1) {
    const sub = h('div', { class: 'xw-pages', role: 'tablist', 'aria-label': `${cur.label} pages` });
    for (const t of cur.tabs) {
      const [e, w] = o.counts(t);
      sub.append(h('button', {
        type: 'button', role: 'tab', class: 'xw-tab' + (t === o.tab ? ' is-active' : ''), 'aria-selected': String(t === o.tab),
        onclick: () => o.go(t),
      }, PAGE_LABELS[t] || t, badge(e, w)));
    }
    nav.append(sub);
  }
  if (PAGE_LEADS[o.tab]) nav.append(h('p', { class: 'xw-page-lead' }, PAGE_LEADS[o.tab]));
}

/** The pack bar's breadcrumb: pack, area, page (and the open map). */
export function crumbs(el, o) {
  el.innerHTML = '';
  const a = areaOf(o.tab);
  const parts = [a.label];
  if (a.tabs.length > 1) parts.push(PAGE_LABELS[o.tab] || o.tab);
  if (o.tab === 'maps' && o.map) parts.push(o.mapName || o.map);
  parts.forEach((p, i) => { if (i) el.append(h('span', { class: 'xw-crumb-sep', 'aria-hidden': 'true' }, ' › ')); el.append(h('span', {}, p)); });
}
