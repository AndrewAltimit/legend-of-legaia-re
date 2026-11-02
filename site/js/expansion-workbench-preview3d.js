/* Expansion workbench: the 3D map preview and the Play here loop.
 *
 * The preview is not a second renderer. It is the play page itself, framed
 * as `play.html?xwpreview=1` (js/play-expansion-live.js on its side): its own
 * LegaiaRuntime reads the disc from the site's shared disc cache, loads the
 * working pack's `.lxpack` and enters the map through the play page's entry
 * path, so the picture is the engine's scene build for that map - its tiles,
 * walls, props, decorations, animators, emitters, materials, lights, mood
 * and fog - drawn by the play page's renderer, with the editor's volumes on
 * top (js/play-view-editor-overlay.js).
 *
 * After an edit (debounced) the working pack is exported again and posted to
 * the frame, which hot-reloads it (`LegaiaRuntime::reload_expansion`: the
 * same activation, the map re-entered with the player where they stood).
 * What is not an edit stays a message and rebuilds nothing: the selection,
 * the overlay layers, the camera, a cutscene's path. An edit that only
 * moved or turned props and NPCs is sent as moves (`xw-move`) and rebuilds
 * nothing either (moveObjects below).
 *
 * Between rebuilds the volumes follow the editor at once: the painter hands
 * over each `map_preview` it computes (`setMapPreview`), and the frame draws
 * the tile grid at its corner heights and a draft of the edited map - its
 * object lists, and the tiles whose walls, void or floor the edit changed -
 * over the engine's reading
 * of the last build, until the rebuild lands and replaces it.
 *
 * The view switch lays the editor out: **3D** (the default: the picture is
 * the working surface, filling the page, with the catalog docked along its
 * bottom and the forms put away), **Split** (the same with the painter and
 * the forms in a drawer beside it) and **2D** (the painter first, the
 * picture a dock above it). The frame is never moved in the page - moving an
 * iframe reloads it - so the layouts are classes on the page (`xw-studio`,
 * `xw-view-split`, `xw-view-3d`) and the style below.
 *
 * Play here posts the same bytes on BroadcastChannel `legaia-expansion-live`:
 * an open play tab on the pack hot-reloads in place and answers; with no
 * answer the bundle goes to the play page's IndexedDB slot (database
 * `legaia-expansion`, store `packs`, key `play`) and a play tab opens on the
 * map (`play.html?expansion=1&map=<id>`).
 */

const CHANNEL = 'legaia-expansion-live';
const LAYERS = [
  ['grid', 'Grid'], ['walls', 'Walls'], ['zones', 'Encounter zones'], ['triggers', 'Triggers + exits'],
  ['npcs', 'NPCs'], ['spawns', 'Spawns'], ['props', 'Props'], ['collision', 'Collision'],
];
const RELOAD_MS = 450;
// The pause after an edit before the preview decides how to show it.
const DECIDE_MS = 30;
const VIEWS = [
  ['3d', '3D', 'The 3D view fills the page, the object catalog along its bottom'],
  ['split', 'Split', 'The 3D view with the tile painter and the forms in a drawer beside it'],
  ['2d', '2D', 'The tile painter and the forms first, with the 3D preview docked above them'],
];
// (A new key: the working surface is the 3D view by default now, whatever
// an older visit left in the old one.)
const VIEW_KEY = 'legaia-xw-view-2';
// The Split view's drawer, as a share of the editor's width.
const SPLIT_KEY = 'legaia-xw-drawer';
const SPLIT_MIN = 0.22, SPLIT_MAX = 0.7;

/* The layouts of the view switch. The page's own style (the HTML) sizes
 * the dock for 2D; these rules only rearrange it, so the page keeps one
 * place for its look. */
const VIEW_CSS = `
/* The studio: the page's own chrome stands back and the editor fills what is
   left of the window, with no page scroll. Rows: navigation, the Build head,
   the picture, the catalog. What is only sometimes needed floats over the
   picture: the View menu, the mode's tool bar, the selection's panel, toasts.
   The drawer (the tab's panel: the painter and the forms) is a column in
   Split, and put away in 3D. */
body.xw-studio .sidebar, body.xw-studio .wb-header, body.xw-studio #xw-setup:not([open]), body.xw-studio .xw-page-lead { display: none; }
body.xw-studio .app { display: block; }
body.xw-studio .content { padding: 0.3rem 0.5rem; max-width: none; }
body.xw-studio #workbench { margin: 0; padding: 0; }
body.xw-studio #xw-main:not([hidden]) { display: flex; flex-direction: column; height: calc(100dvh - var(--topbar-h, 0px) - 0.6rem); min-height: 28rem; }
body.xw-studio .wb-toolbar-pack { position: static; flex: none; flex-wrap: nowrap; padding: 0.15rem 0.5rem; margin: 0 0 0.25rem; box-shadow: none; }
body.xw-studio .wb-toolbar-pack .wb-actions { margin: 0; gap: 0.25rem; flex-wrap: nowrap; flex: none; position: relative; }
body.xw-studio .wb-toolbar-pack .wb-button { padding: 0.15rem 0.6rem; }
body.xw-studio .xw-packname { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
body.xw-studio .xw-crumbs, body.xw-studio #xw-saved { display: none; }
body.xw-studio .xw-more-btn { display: inline-block; align-self: stretch; }
body.xw-studio .xw-more-items { display: none; }
body.xw-studio .xw-more-items.is-open { display: flex; flex-direction: column; align-items: stretch; gap: 0.35rem; position: absolute; z-index: 40; right: 0; top: calc(100% + 0.3rem); min-width: 12rem; padding: 0.5rem; background: var(--bg-raised); border: 1px solid var(--border-hi); border-radius: 7px; box-shadow: 0 8px 22px rgba(0, 0, 0, 0.4); }
body.xw-studio .xw-checks-btn.is-warn { border-color: var(--border); color: var(--text-muted); }
body.xw-studio .xw-areas { padding: 0.2rem 0.3rem; }
body.xw-studio .xw-area { padding: 0.12rem 0.6rem; font-size: 0.84em; }
body.xw-studio .xw-work { flex: 1 1 auto; min-height: 0; grid-template-rows: minmax(0, 1fr); }
body.xw-studio .xw-edpane { display: grid; grid-template-columns: minmax(0, 1fr); grid-template-rows: auto auto minmax(0, 1fr) auto; height: 100%; min-height: 0; overflow: hidden; position: relative; }
body.xw-studio.xw-view-split .xw-edpane { grid-template-columns: minmax(0, 1fr) var(--xw-split-col, min(28rem, 40%)); }
body.xw-studio .xw-edpane > .xw-nav { grid-column: 1 / -1; grid-row: 1; }
body.xw-studio .xw-edpane > .xw-build { grid-column: 1 / -1; grid-row: 2; }
body.xw-studio .xw-edpane > .xw-3d { grid-column: 1; grid-row: 3; min-height: 0; padding: 0; position: relative; border-bottom: 0; }
body.xw-studio .xw-edpane > .xw-catdock { grid-column: 1; grid-row: 4; }
body.xw-studio .xw-catalog { padding: 0.25rem 0.4rem 0.1rem; border-bottom: 0; }
body.xw-studio .xw-edpane > .xw-panel { display: none; }
body.xw-studio.xw-view-split .xw-edpane > .xw-panel { display: block; grid-column: 2; grid-row: 3 / span 2; min-height: 0; overflow: auto; border-left: 1px solid var(--border); border-radius: 0; }
/* The picture fills its row; the tool bars float on it. */
body.xw-studio .xw-3d-body { position: absolute; inset: 0; margin: 0; }
body.xw-studio .xw-3d-body .xw-3d-frame { position: absolute; inset: 0; width: 100%; height: 100%; max-width: none; aspect-ratio: auto; margin: 0; border: 0; border-radius: 0; }
body.xw-studio .xw-3d-head { position: absolute; z-index: 7; left: 0.5rem; top: 0.5rem; flex-wrap: nowrap; white-space: nowrap; }
body.xw-studio .xw-3d-editslot { position: absolute; z-index: 6; left: 4.2rem; top: 0.5rem; right: 15rem; margin: 0; }
body.xw-studio .xw-3d-title, body.xw-studio .xw-3d-toggle, body.xw-studio .xw-3d-ctrl-mode, body.xw-studio .xw-3d-edit > .wb-label, body.xw-studio .xw-3d-edit > .xw-hint, body.xw-studio .xw-3d-keys, body.xw-studio .xw-3d-edit > .xw-layout, body.xw-studio .xw-3d-modebtn, body.xw-studio .xw-3d-seltools, body.xw-studio .xw-3d-edit > .xw-sep { display: none; }
body.xw-studio .xw-3d-edit { margin: 0; }
/* A control shows in the modes where it does something. */
.xw-3d-group { display: inline-flex; flex-wrap: wrap; align-items: center; gap: 0.25rem 0.4rem; }
body.xw-studio .xw-3d-edit:not([data-bm="terrain"]) .xw-only-terrain, body.xw-studio .xw-3d-edit:not([data-bm="build"]) .xw-only-build { display: none; }
body.xw-studio .xw-3d-edit:not([data-bm="terrain"]) > :not(.xw-3d-group) { display: none; }
/* Toasts: over the picture, moving nothing. */
body.xw-studio .xw-3d-status { position: absolute; z-index: 8; left: 50%; bottom: 0.6rem; transform: translateX(-50%); flex: none; max-width: 80%; padding: 0.15rem 0.7rem; border-radius: 999px; background: rgba(8, 12, 18, 0.8); color: #e6ecf2; pointer-events: none; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
body.xw-studio .xw-3d-status:empty { display: none; }
body.xw-studio .xw-3d-status.is-err { background: rgba(92, 16, 16, 0.92); color: #fff; white-space: normal; }
body.xw-studio #xw-status:not(.is-err) { display: none; }
body.xw-studio #xw-status, body.xw-studio #xw-export-status { position: fixed; z-index: 1100; left: 50%; bottom: 11rem; transform: translateX(-50%); max-width: 70vw; margin: 0; padding: 0.15rem 0.7rem; border-radius: 999px; background: rgba(8, 12, 18, 0.8); color: #e6ecf2; font-family: inherit; pointer-events: none; }
/* The catalog's handle, on the picture's bottom edge. */
.xw-catdock { position: relative; }
.xw-catdock[hidden] { display: none; }
.xw-cat-toggle { display: none; }
body.xw-studio .xw-cat-toggle { display: block; position: absolute; z-index: 9; right: 0.5rem; top: -1.7rem; }
body.xw-cat-closed .xw-catalog { display: none; }
/* Fullscreen: the picture edge to edge and everything else floating on it. */
.xw-fs-menu-btn { display: none; }
body.xw-fs #xw-main:not([hidden]) { position: fixed; inset: 0; z-index: 1000; display: block; height: auto; min-height: 0; padding: 0; background: var(--bg, #0b0d12); }
body.xw-fs .xw-work { display: block; height: 100%; }
body.xw-fs.xw-studio .xw-edpane { display: block; height: 100%; border: 0; border-radius: 0; }
body.xw-fs.xw-studio .xw-edpane > .xw-3d { position: absolute; inset: 0; }
body.xw-fs .wb-toolbar-pack, body.xw-fs.xw-studio .xw-build, body.xw-fs .xw-fs-menu-btn, body.xw-fs.xw-studio .xw-catdock .xw-catalog, body.xw-fs .xw-nav { background: color-mix(in srgb, var(--bg-raised) 78%, transparent); -webkit-backdrop-filter: blur(6px); backdrop-filter: blur(6px); border: 1px solid color-mix(in srgb, var(--border-hi) 60%, transparent); }
body.xw-fs.xw-studio .wb-toolbar-pack { position: absolute; z-index: 30; top: 0.4rem; right: 0.4rem; margin: 0; padding: 0.15rem 0.25rem; border-radius: 9px; }
body.xw-fs .wb-toolbar-pack .xw-packname, body.xw-fs #xw-export-status:empty { display: none; }
body.xw-fs .xw-fs-menu-btn { display: block; position: absolute; z-index: 32; top: 0.4rem; left: 0.4rem; padding: 0.15rem 0.5rem; border-radius: 9px; color: var(--text-bright); font: inherit; font-size: 1rem; cursor: pointer; }
body.xw-fs.xw-studio .xw-edpane > .xw-nav { display: none; position: absolute; z-index: 31; top: 2.5rem; left: 0.4rem; max-width: min(36rem, 90vw); border-radius: 9px; }
body.xw-fs.xw-fs-menu.xw-studio .xw-edpane > .xw-nav { display: block; }
body.xw-fs.xw-studio .xw-edpane > .xw-build { position: absolute; z-index: 20; top: 0.4rem; left: 2.9rem; max-width: calc(100% - 17rem); flex-wrap: nowrap; padding: 0.12rem 0.3rem; border-radius: 9px; }
body.xw-fs .xw-mode { background: transparent; border-color: transparent; }
body.xw-fs .xw-mode:not(.is-on) .xw-mode-label { display: none; }
body.xw-fs .xw-mode.is-on { background: var(--accent-soft); border-color: var(--accent); }
body.xw-fs .xw-build-tools { margin-left: 0.3rem; }
body.xw-fs.xw-studio .xw-3d-head { top: 2.9rem; left: 0.4rem; }
body.xw-fs.xw-studio .xw-3d-editslot { top: 2.9rem; left: 4rem; }
body.xw-fs.xw-studio .xw-edpane > .xw-catdock { position: absolute; z-index: 15; left: 0.4rem; right: 0.4rem; bottom: 0.4rem; }
body.xw-fs.xw-studio .xw-catdock .xw-catalog { border-radius: 9px; }
body.xw-fs.xw-studio.xw-view-split .xw-edpane > .xw-panel { position: absolute; z-index: 18; top: 2.9rem; right: 0.4rem; bottom: 0.4rem; width: min(28rem, 44%); border: 1px solid var(--border-hi); border-radius: 9px; }
body.xw-fs.xw-view-split .xw-edpane > .xw-split-handle { display: none; }
body.xw-fs .xw-issues { position: absolute; z-index: 25; top: 2.9rem; right: 0.4rem; width: 22rem; max-height: 60%; }
body.xw-fs .xw-selpanel, body.xw-fs .xw-float-pill { top: 2.9rem; }
body.xw-fs .xw-firsthint { bottom: 11rem; }
body.xw-fs.xw-studio .xw-3d-status { bottom: 11rem; }
body.xw-fs.xw-cat-closed .xw-firsthint, body.xw-fs.xw-cat-closed.xw-studio .xw-3d-status { bottom: 2.6rem; }
body.xw-fs.xw-studio .xw-cat-toggle { top: -1.8rem; right: 0; }
body.xw-fs.xw-cat-closed.xw-studio .xw-edpane > .xw-catdock { bottom: 2.3rem; }
/* The pack bar's words become small icons; their tooltips say the rest. */
body.xw-fs [data-ic] { font-size: 0; padding: 0.1rem 0.45rem; min-width: 1.8rem; line-height: 1.5; }
body.xw-fs [data-ic]::before { content: attr(data-ic); font-size: 0.95rem; font-weight: 600; }
body.xw-split-drag, body.xw-split-drag * { cursor: col-resize !important; user-select: none; }
body.xw-split-drag .xw-3d-frame { pointer-events: none; }
.xw-split-handle { display: none; }
body.xw-studio.xw-view-split .xw-edpane > .xw-split-handle { display: block; grid-column: 2; grid-row: 3 / span 2; justify-self: start; z-index: 4; width: 0.6rem; margin-left: -0.3rem; cursor: col-resize; touch-action: none; background: none; border: 0; padding: 0; position: relative; }
.xw-split-handle::after { content: ''; position: absolute; left: calc(50% - 1px); top: 40%; height: 20%; width: 3px; border-radius: 2px; background: var(--border-hi); }
.xw-split-handle:hover::after, .xw-split-handle:focus-visible::after, .xw-split-handle.is-drag::after { background: var(--accent); }
.xw-3d-views { display: inline-flex; gap: 0; }
.xw-3d-views .wb-mini { border-radius: 0; margin: 0; }
.xw-3d-views .wb-mini:first-child { border-radius: 4px 0 0 4px; }
.xw-3d-views .wb-mini:last-child { border-radius: 0 4px 4px 0; }
/* The View menu: a popover over the picture. */
.xw-3d-more { position: relative; display: inline-block; }
.xw-3d-more > summary { list-style: none; cursor: pointer; }
.xw-3d-more > summary::-webkit-details-marker { display: none; }
.xw-3d-more-box { position: absolute; z-index: 20; top: calc(100% + 0.2rem); left: 0; width: max-content; max-width: min(24rem, 80vw); display: flex; flex-wrap: wrap; align-items: center; gap: 0.3rem 0.6rem; padding: 0.5rem 0.6rem; background: var(--bg-raised); border: 1px solid var(--border-hi); border-radius: 7px; box-shadow: 0 8px 22px rgba(0, 0, 0, 0.45); white-space: normal; }
.xw-3d-more-box .xw-check { display: inline-flex; flex-direction: row; gap: 0.25rem; color: var(--text-muted); }
/* What floats over the picture: the piece being placed, the selection, a first-run hint. */
.xw-float { position: absolute; z-index: 5; inset: 0; pointer-events: none; }
.xw-float > * { pointer-events: auto; }
.xw-float-in { pointer-events: none; }
.xw-float-in > * { pointer-events: auto; }
.xw-float-pill { position: absolute; left: 50%; top: 0.5rem; transform: translateX(-50%); max-width: 92%; padding: 0.2rem 0.75rem; border-radius: 999px; background: rgba(8, 12, 18, 0.82); color: #eef3f8; font-size: 0.82rem; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; pointer-events: none; }
.xw-float-pill.is-bad { background: rgba(92, 16, 16, 0.9); }
.xw-float-pill kbd { font: inherit; font-weight: 700; color: #8fc2ff; }
.xw-selpanel { position: absolute; right: 0.5rem; top: 0.5rem; width: 14rem; padding: 0.45rem; border: 1px solid var(--border-hi); border-radius: 8px; background: color-mix(in srgb, var(--bg-raised) 93%, transparent); box-shadow: 0 6px 18px rgba(0, 0, 0, 0.4); font-size: 0.84rem; }
.xw-selpanel-head { display: flex; align-items: center; gap: 0.45rem; margin-bottom: 0.35rem; min-width: 0; }
.xw-selpanel-head img { width: 2.6rem; height: 2.6rem; border-radius: 5px; flex: none; }
.xw-selpanel-head strong { color: var(--text-bright); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.xw-selpanel-btns { display: grid; grid-template-columns: 1fr 1fr; gap: 0.25rem; }
.xw-selpanel-btns .wb-mini { padding: 0.2rem 0.3rem; }
.xw-selpanel-btns .is-danger:hover { border-color: var(--open); color: var(--open); }
.xw-selpanel details { margin-top: 0.3rem; }
.xw-selpanel summary { cursor: pointer; color: var(--text-muted); font-size: 0.9em; }
.xw-firsthint { position: absolute; left: 0.6rem; bottom: 2.2rem; max-width: min(23rem, 80%); padding: 0.55rem 0.7rem; border: 1px solid var(--accent-line); border-radius: 8px; background: rgba(8, 12, 18, 0.88); color: #eef3f8; font-size: 0.86rem; line-height: 1.45; }
.xw-firsthint strong { color: #fff; }
.xw-firsthint .wb-mini { margin-top: 0.35rem; }
@media (max-width: 44rem) {
  /* A phone, out of fullscreen: the page scrolls again: the picture, the
     catalog under it, the drawer's forms under that. */
  body.xw-studio:not(.xw-fs) #xw-main:not([hidden]) { display: block; height: auto; }
  body.xw-studio:not(.xw-fs) .wb-toolbar-pack, body.xw-studio:not(.xw-fs) .wb-toolbar-pack .wb-actions { flex-wrap: wrap; }
  body.xw-studio:not(.xw-fs) .wb-toolbar-pack .wb-actions { flex: 1 1 auto; }
  body.xw-studio:not(.xw-fs) .xw-edpane, body.xw-studio.xw-view-split:not(.xw-fs) .xw-edpane { display: block; height: auto; overflow: visible; }
  body.xw-studio:not(.xw-fs) .xw-edpane > .xw-3d { height: 58vh; }
  body.xw-studio .xw-3d-editslot { right: 0.5rem; }
  body.xw-studio.xw-view-split:not(.xw-fs) .xw-edpane > .xw-panel { border-left: 0; overflow: visible; }
  body.xw-studio.xw-view-split .xw-edpane > .xw-split-handle { display: none; }
  body.xw-studio:not(.xw-fs) .xw-cat-toggle { display: none; }
  body.xw-fs.xw-studio .xw-edpane > .xw-build { flex-wrap: wrap; max-width: calc(100% - 3.4rem); }
  body.xw-fs.xw-studio .wb-toolbar-pack { top: auto; bottom: 0.4rem; right: 0.4rem; }
  .xw-selpanel { width: 11.5rem; }
}
`;

function installViewCss() {
  if (document.getElementById('xw-view-css')) return;
  const st = document.createElement('style');
  st.id = 'xw-view-css';
  st.textContent = VIEW_CSS;
  document.head.append(st);
}

/* The tile grid's corner heights (world units up) for the frame: the
 * synthetic image's tiers, or on a terrain map its heightfield sampled at
 * each tile corner as \`Heightfield::sample_world\` does (bilinear here: a
 * corner sits on a heightfield vertex). */
export function gridCorners(map, pv) {
  if (!pv || pv.error || !pv.corners) return null;
  const W = pv.width, H = pv.height;
  const t = map && map.terrain ? pv.terrain : null;
  // Ramps are filled on a tier map; a terrain map slopes everywhere and
  // shows its relief itself.
  if (!t || !t.heights || !t.origin) return { width: W, height: H, corners: pv.corners, slopes: true };
  const o0 = (map.tiles && map.tiles.origin) || [0, 0];
  const at = (i, j) => t.heights[Math.max(0, Math.min(t.rows - 1, j)) * t.cols + Math.max(0, Math.min(t.cols - 1, i))];
  const sample = (x, z) => {
    const fi = (x - t.origin[0]) / t.step, fj = (z - t.origin[1]) / t.step;
    const i = Math.floor(fi), j = Math.floor(fj), u = fi - i, v = fj - j;
    return Math.round((at(i, j) * (1 - u) + at(i + 1, j) * u) * (1 - v) + (at(i, j + 1) * (1 - u) + at(i + 1, j + 1) * u) * v);
  };
  const corners = new Array(W * H);
  for (let r = 0; r < H; r++) {
    for (let c = 0; c < W; c++) {
      const x0 = (o0[0] + c) * 128, z0 = (o0[1] + r) * 128;
      corners[r * W + c] = [sample(x0, z0), sample(x0 + 128, z0), sample(x0, z0 + 128), sample(x0 + 128, z0 + 128)];
    }
  }
  return { width: W, height: H, corners, slopes: false };
}

/* The draft of the edited map for the frame: the object lists as the
 * definition holds them, and the tiles whose walls, void or floor changed
 * since \`built\` (the \`map_preview\` of the pack the last rebuild was sent),
 * as \`[index, walls, void, floor]\` - only those, so a tile the edit left
 * alone keeps the engine's own reading (its sampled floor, its walls). A
 * changed tile's floor is its corners' mean (world units up); a terrain
 * map's floor is its heightfield, which the engine samples, so there only
 * walls and void are drafted. */
export function draftDefs(map, pv, built) {
  if (!map || !pv || pv.error) return null;
  const d = { width: pv.width, height: pv.height };
  for (const k of ['spawns', 'exits', 'triggers', 'encounters', 'props', 'npcs']) d[k] = map[k] || [];
  const cells = [];
  if (built && !built.error && built.width === pv.width && built.height === pv.height) {
    const terrain = !!map.terrain;
    for (let i = 0; i < pv.width * pv.height; i++) {
      const q = pv.corners[i], b = built.corners[i];
      const moved = !terrain && (q[0] !== b[0] || q[1] !== b[1] || q[2] !== b[2] || q[3] !== b[3]);
      if (pv.walls[i] === built.walls[i] && !!pv.void[i] === !!built.void[i] && !moved) continue;
      cells.push([i, pv.walls[i], pv.void[i] ? 1 : 0, moved ? Math.round((q[0] + q[1] + q[2] + q[3]) / 4) : null]);
    }
  }
  d.cells = cells;
  return d;
}

/* Object-only edits. The pack as JSON with the open map's object poses
 * blanked - each prop's tile, offset and turn, each NPC's tile and facing:
 * two packs that differ only in those give the same text, and the
 * preview moves the drawn objects instead of rebuilding the map. A prop
 * resting `on` another, or one another rests on, keeps its pose: where it
 * stands sets a height the engine resolves when it builds the map (the
 * support's top under the item), so moving it rebuilds. */
export function strippedJson(pack, mapId) {
  const m = pack && pack.maps && pack.maps[mapId];
  if (!m) return JSON.stringify(pack);
  const props = m.props, npcs = m.npcs, spawns = m.spawns, lighting = m.lighting;
  const rested = new Set();
  for (const x of (props || []).concat(m.decor || [])) if (x.on) rested.add(x.on);
  const keeps = (p) => !!p.on || (p.id && rested.has(p.id));
  m.props = (props || []).map((p) => (keeps(p) ? p : Object.assign({}, p, { tile: null, offset: p.offset ? [0, p.offset[1] || 0, 0] : null, rotation: p.rotation ? [p.rotation[0] || 0, 0, p.rotation[2] || 0] : null })));
  m.npcs = (npcs || []).map((n) => Object.assign({}, n, { tile: null, facing: null }));
  m.spawns = (spawns || []).map((n) => Object.assign({}, n, { tile: null, facing: null }));
  if (lighting && lighting.lamps) m.lighting = Object.assign({}, lighting, { lamps: lighting.lamps.map((l) => Object.assign({}, l, { tile: null, offset: null })) });
  try { return JSON.stringify(pack); } finally { m.props = props; m.npcs = npcs; m.spawns = spawns; m.lighting = lighting; }
}

/* Each prop's, NPC's, spawn's and lamp's pose on `map`, in the terms a
 * move is sent in. */
export function objectPoses(map) {
  const o0 = (map.tiles && map.tiles.origin) || [0, 0];
  const world = (t, off) => [(o0[0] + t[0]) * 128 + 64 + ((off && off[0]) || 0), (o0[1] + t[1]) * 128 + 64 + ((off && off[2]) || 0)];
  return {
    props: (map.props || []).map((p) => ({ tile: p.tile || [0, 0], offset: p.offset || [0, 0, 0], at: world(p.tile || [0, 0], p.offset), yaw: (p.rotation || [0, 0, 0])[1] || 0 })),
    npcs: (map.npcs || []).map((n) => ({ tile: n.tile || [0, 0], facing: n.facing || 0 })),
    spawns: (map.spawns || []).map((n) => ({ tile: n.tile || [0, 0], facing: n.facing || 0 })),
    lamps: ((map.lighting && map.lighting.lamps) || []).map((l) => ({ tile: l.tile || [0, 0], offset: l.offset || [0, 96, 0] })),
  };
}

/* The moves from poses `a` to `b` (same lists): props by world position,
 * turn and the floor's rise between their tiles (`grid`, the corner
 * heights the frame's tile grid draws), with the new tile and offset the
 * engine's copy takes; NPCs and spawns by tile and facing; lamps by tile
 * and offset. `null` when a prop changes tile with no grid to lift it by. */
export function objectMoves(a, b, grid) {
  const props = [];
  const npcs = [];
  const spawns = [];
  const lamps = [];
  const floor = (t) => {
    const q = grid.corners[t[1] * grid.width + t[0]];
    return q ? (q[0] + q[1] + q[2] + q[3]) / 4 : 0;
  };
  const same = (x, y) => JSON.stringify(x) === JSON.stringify(y);
  for (let i = 0; i < b.props.length; i++) {
    const p = a.props[i], q = b.props[i];
    if (p.at[0] === q.at[0] && p.at[1] === q.at[1] && p.yaw === q.yaw) continue;
    const moved = p.tile[0] !== q.tile[0] || p.tile[1] !== q.tile[1];
    if (moved && !grid) return null;
    props.push({ i, from: p.at, to: q.at, turn: q.yaw - p.yaw, dy: moved ? Math.round(floor(q.tile) - floor(p.tile)) : 0, tile: q.tile, offset: q.offset, yaw: q.yaw });
  }
  for (let n = 0; n < b.npcs.length; n++) {
    const p = a.npcs[n], q = b.npcs[n];
    if (p.tile[0] === q.tile[0] && p.tile[1] === q.tile[1] && p.facing === q.facing) continue;
    npcs.push({ n, tile: q.tile, facing: q.facing });
  }
  for (let n = 0; n < (b.spawns || []).length; n++) {
    if (!same(a.spawns[n], b.spawns[n])) spawns.push({ n, tile: b.spawns[n].tile, facing: b.spawns[n].facing });
  }
  for (let n = 0; n < (b.lamps || []).length; n++) {
    if (!same(a.lamps[n], b.lamps[n])) lamps.push({ n, tile: b.lamps[n].tile, offset: b.lamps[n].offset });
  }
  return { props, npcs, spawns, lamps };
}

export function createPreview3d(env) {
  // env: { exportBundle() -> Uint8Array (throws on errors), pack(), storePlay(bytes, map) -> Promise, playUrl(map) }
  const P = {
    open: false, ready: false, visible: false, frame: null, map: null, spawn: '',
    dirty: true, timer: null, sent: null, layers: { grid: false, walls: false, zones: false, triggers: false, npcs: false, spawns: false, props: false, collision: false },
    select: null, path: null, marker: null, mode: 'orbit', onPick: null, stats: null, last: null, live: false, liveTimer: null,
    auto: true, painter: true, arrival: false, edits: 0, builtEdits: -1, view: '3d', grid: null, defs: null, overlayKey: '',
  };
  P.view = '3d';
  try { const kept = localStorage.getItem(VIEW_KEY); if (VIEWS.some(([k]) => k === kept)) P.view = kept; } catch (e) { /* no storage: the default */ }
  installViewCss();
  let channel = null;
  try { channel = new BroadcastChannel(CHANNEL); } catch (e) { channel = null; }

  const el = document.createElement('div');
  el.className = 'xw-3d';
  el.hidden = true;
  const head = document.createElement('div');
  head.className = 'xw-3d-head';
  const body = document.createElement('div');
  body.className = 'xw-3d-body';
  body.hidden = true;
  const status = document.createElement('div');
  status.className = 'xw-3d-status';
  el.append(head, body);

  /* The Split view's divider: drag it (or arrow keys on it) to give the
   * picture or the forms more room; double-click resets. The share is
   * kept per browser. It sits in the editor's grid beside the picture -
   * the frame itself never moves. */
  const handle = document.createElement('button');
  handle.type = 'button';
  handle.className = 'xw-split-handle';
  handle.title = 'Drag to resize the 3D view (arrow keys too; double-click resets)';
  handle.setAttribute('role', 'separator');
  handle.setAttribute('aria-orientation', 'vertical');
  handle.setAttribute('aria-label', 'Resize the 3D view');
  let split = null;
  try { const f = parseFloat(localStorage.getItem(SPLIT_KEY)); if (f >= SPLIT_MIN && f <= SPLIT_MAX) split = f; } catch (e) { split = null; }
  function applySplit() {
    const pane = el.parentElement;
    if (!pane) return;
    if (split == null) pane.style.removeProperty('--xw-split-col');
    else pane.style.setProperty('--xw-split-col', `${(split * 100).toFixed(1)}%`);
    handle.setAttribute('aria-valuenow', String(Math.round((split == null ? 0.565 : split) * 100)));
    if (handle.parentElement !== pane) el.before(handle);
  }
  function setSplit(f, keep) {
    split = f == null ? null : Math.max(SPLIT_MIN, Math.min(SPLIT_MAX, f));
    applySplit();
    if (keep) { try { if (split == null) localStorage.removeItem(SPLIT_KEY); else localStorage.setItem(SPLIT_KEY, split.toFixed(3)); } catch (e) { /* no storage */ } }
  }
  handle.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    handle.setPointerCapture(e.pointerId);
    handle.classList.add('is-drag');
    document.body.classList.add('xw-split-drag');
  });
  handle.addEventListener('pointermove', (e) => {
    if (!handle.hasPointerCapture(e.pointerId)) return;
    const r = el.parentElement.getBoundingClientRect();
    setSplit((r.right - e.clientX) / Math.max(1, r.width), false);
  });
  const endDrag = (e) => {
    if (!handle.classList.contains('is-drag')) return;
    handle.classList.remove('is-drag');
    document.body.classList.remove('xw-split-drag');
    if (handle.hasPointerCapture(e.pointerId)) handle.releasePointerCapture(e.pointerId);
    setSplit(split, true);
  };
  handle.addEventListener('pointerup', endDrag);
  handle.addEventListener('pointercancel', endDrag);
  handle.addEventListener('dblclick', () => setSplit(null, true));
  handle.addEventListener('keydown', (e) => {
    const d = e.key === 'ArrowLeft' ? 0.03 : e.key === 'ArrowRight' ? -0.03 : 0;
    if (!d) return;
    e.preventDefault();
    setSplit((split == null ? 0.565 : split) + d, true);
  });

  const btn = (label, title, fn) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'wb-mini';
    b.textContent = label;
    if (title) b.title = title;
    b.addEventListener('click', fn);
    return b;
  };
  const toggleBtn = btn('Open 3D preview', 'Render this map as the engine builds it, in the play page framed here. It reads your disc from the site\'s disc cache: the first open takes a few seconds.', () => setOpen(!P.open));
  /* The editor's camera has no player on the map; Test play is the one
   * way the party enters the picture (and the game's keys drive it). */
  const orbitBtn = btn('Edit', 'The editor camera: right drag orbits, middle drag (or Space + drag) pans, the wheel zooms; T looks straight down, F frames the selection. No player on the map.', () => setMode('orbit'));
  const playerBtn = btn('Test play', 'Put the party on the map - on the selected tile, else at the first spawn - and play it with the game\'s keys and field camera. Edit (or this button) comes back.', () => testPlay(P.mode !== 'player'));
  const topBtn = btn('Top view', 'Look straight down on the map (T); again to tilt back', () => {
    if (P.mode !== 'orbit') setMode('orbit');
    P.top = !P.top;
    topBtn.classList.toggle('is-on', P.top);
    post({ type: 'xw-camera', mode: 'orbit', painter: P.painter, top: P.top });
  });
  const fitBtn = btn('Fit map', 'Frame the whole map', () => {
    setMode('orbit', true);
    post({ type: 'xw-camera', mode: 'orbit', focus: null, painter: P.painter });
  });
  /* Test play on or off: on stands the player on the selected tile (or
   * the first spawn) and switches to the field camera. */
  function testPlay(on) {
    if (!on) { setMode('orbit'); return; }
    const t = selTile() || firstSpawn();
    rebuildIfStale();
    if (t) post({ type: 'xw-seat', tile: t });
    setMode('player');
  }
  function firstSpawn() {
    const m = P.map && pack() && pack().maps && pack().maps[P.map];
    const s = m && (m.spawns || [])[0];
    return s && s.tile ? s.tile : null;
  }
  const reloadBtn = btn('Reload', 'Rebuild the map from the pack now', () => { P.dirty = true; P.forceFull = true; flush(); });
  const layerBox = document.createElement('span');
  layerBox.className = 'xw-3d-layers';
  for (const [k, label] of LAYERS) {
    const lab = document.createElement('label');
    lab.className = 'xw-check';
    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.checked = !!P.layers[k];
    cb.dataset.layer = k;
    if (k === 'collision') lab.title = 'What the player bumps into, read from the engine\'s collision grid after the last build: red where solid props, decorations or terrain close the ground beyond the painted walls';
    cb.addEventListener('change', () => { P.layers[k] = cb.checked; sendOverlay(); });
    lab.append(cb, document.createTextNode(label));
    layerBox.append(lab);
  }
  toggleBtn.classList.add('xw-3d-toggle');
  const title = document.createElement('strong');
  title.className = 'xw-3d-title';
  title.textContent = '3D preview';
  const viewBox = document.createElement('span');
  viewBox.className = 'xw-3d-views';
  viewBox.setAttribute('role', 'group');
  viewBox.setAttribute('aria-label', 'Map view');
  const viewBtns = {};
  for (const [k, label, tip] of VIEWS) {
    const b = btn(label, tip, () => setView(k));
    b.dataset.view = k;
    viewBtns[k] = b;
    viewBox.append(b);
  }
  const ctrls = document.createElement('span');
  ctrls.className = 'xw-3d-ctrls';
  ctrls.hidden = true;
  const autoLab = document.createElement('label');
  autoLab.className = 'xw-check';
  autoLab.title = 'Rebuild after each edit (off: only Reload rebuilds - for a big map)';
  const autoCb = document.createElement('input');
  autoCb.type = 'checkbox';
  autoCb.checked = true;
  autoCb.addEventListener('change', () => { P.auto = autoCb.checked; if (P.auto && P.dirty) schedule(); });
  autoLab.append(autoCb, document.createTextNode('Auto'));
  const check = (label, title, on, fn) => {
    const lab = document.createElement('label');
    lab.className = 'xw-check';
    lab.title = title;
    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.checked = on;
    cb.addEventListener('change', () => fn(cb.checked));
    lab.append(cb, document.createTextNode(label));
    return lab;
  };
  /* The game looks at a map from its low-Z side with the world's own
   * handedness, which is the painter's grid mirrored (the painter draws row 0
   * at the top). Match painter turns the orbit camera to the high-Z side and
   * mirrors the picture, so the 3D view reads as the grid does: row 0 at the
   * top, columns left to right. The Player camera is always the game's. */
  const painterLab = check('Match painter', 'Orbit view oriented as the painter draws the grid (row 0 at the top, columns left to right); off shows the map with the game\'s own handedness', true, (on) => {
    P.painter = on;
    post({ type: 'xw-camera', mode: P.mode, painter: on });
  });
  /* A preview entry skips the map's arrival cutscene and place-name banner;
   * ticked, the map is entered again as a player arrives. */
  const arrivalLab = check('Arrival', 'Play the map\'s arrival cutscene and name banner on entry, as a player arriving sees them (re-enters the map at its spawn)', false, (on) => {
    P.arrival = on;
    post({ type: 'xw-arrival', on });
    if (on && P.sent) { P.sent = null; P.dirty = true; flush(); }
  });
  /* What most sessions never touch folds under one button. */
  const more = document.createElement('details');
  more.className = 'xw-3d-more';
  const moreSum = document.createElement('summary');
  moreSum.className = 'wb-mini';
  moreSum.textContent = 'View';
  moreSum.title = 'The layout, the camera, layers over the picture (grid, walls, zones, collision), and how the preview rebuilds';
  const moreBox = document.createElement('div');
  moreBox.className = 'xw-3d-more-box';
  layerBox.classList.add('xw-3d-more-box-layers');
  const debugLab = check('Build timings', 'Show how long each rebuild takes and the frame\'s cost over the picture', false, (on) => { P.debug = on; paintStatus(); });
  moreBox.append(viewBox, topBtn, fitBtn, layerBox, autoLab, painterLab, arrivalLab, reloadBtn, debugLab);
  more.append(moreSum, moreBox);
  // The mode toolbar's Test-play is the way into a test on the Build area.
  orbitBtn.classList.add('xw-3d-ctrl-mode');
  playerBtn.classList.add('xw-3d-ctrl-mode');
  ctrls.append(orbitBtn, playerBtn, more);
  document.addEventListener('click', (ev) => { if (more.open && !more.contains(ev.target)) more.open = false; });
  // The map editor's bar (expansion-workbench-gizmos.js), under the head.
  const editSlot = document.createElement('div');
  editSlot.className = 'xw-3d-editslot';
  head.append(title, toggleBtn, ctrls);
  el.append(status);
  body.before(editSlot);
  layerBox.hidden = true;
  /* Over the picture: the editor's panels (setFloat). */
  const float = document.createElement('div');
  float.className = 'xw-float';
  body.append(float);

  /* The status line: the last build's or action's message, then the
   * frame's cost (refreshed each second without losing the message). */
  let msg = '';
  let msgBad = false;
  let devMsg = '';
  let msgTimer = null;
  /* `dev` is developer text (a build's or a move's time): it replaces nothing
   * the modder needs and shows only with Build timings on, or in the 2D view's
   * status line. Anything else over the picture is a toast: it goes by itself
   * unless it is a problem, or `hold` keeps it (the engine still starting). */
  function setStatus(text, bad, how) {
    clearTimeout(msgTimer);
    if (how === 'dev') {
      devMsg = text || '';
      status.dataset.dev = devMsg;
      msg = '';
      msgBad = false;
    } else {
      msg = text || '';
      msgBad = !!bad;
      if (msg && !bad && how !== 'hold') msgTimer = setTimeout(() => { msg = ''; paintStatus(); }, 3500);
    }
    paintStatus();
  }
  function paintStatus() {
    const shown = P.debug || P.view === '2d';
    const dev = shown ? devMsg + (P.stats && P.open ? statsText() : '') : '';
    status.textContent = msg ? msg + (dev ? ` ${dev}` : '') : dev;
    status.classList.toggle('is-err', msgBad);
  }

  function selTile() {
    const s = P.select;
    if (!s) return null;
    if (s.tile) return s.tile;
    if (s.rect) return [Math.min(s.rect[0], s.rect[2]), Math.min(s.rect[1], s.rect[3])];
    return null;
  }

  /* The picture's backdrop while editing (never in Test play or Play): a calm
   * neutral of the page's theme, read off the page so it follows the toggle. */
  function backdrop() {
    const c = document.createElement('canvas').getContext('2d');
    c.fillStyle = getComputedStyle(document.body).backgroundColor;
    c.fillRect(0, 0, 1, 1);
    const [r, g, b] = c.getImageData(0, 0, 1, 1).data;
    const dark = 0.299 * r + 0.587 * g + 0.114 * b < 128;
    return dark ? [0.17, 0.19, 0.22, 1] : [0.78, 0.8, 0.83, 1];
  }
  function sendBackdrop() { post({ type: 'xw-backdrop', color: backdrop() }); }
  new MutationObserver(sendBackdrop).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'class'] });
  if (window.matchMedia) window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', sendBackdrop);

  function post(msg, transfer) {
    if (P.frame && P.frame.contentWindow && P.ready) {
      P.frame.contentWindow.postMessage(msg, location.origin, transfer || []);
    }
  }

  function setMode(mode, refit) {
    P.mode = mode;
    if (mode === 'player') rebuildIfStale();
    orbitBtn.classList.toggle('is-on', mode === 'orbit');
    playerBtn.classList.toggle('is-on', mode === 'player');
    playerBtn.textContent = mode === 'player' ? 'Stop test' : 'Test play';
    if (P.onMode) P.onMode(mode);
    if (!refit) post({ type: 'xw-camera', mode, painter: P.painter });
    if (mode === 'player' && P.frame) P.frame.focus();
  }

  /* The layout: 2D, Split or 3D (see the header). Split and 3D open the
   * preview; the choice is kept per browser. Off the Maps tab the page
   * stays as 2D lays it out. */
  function setView(view) {
    P.view = view;
    try { localStorage.setItem(VIEW_KEY, view); } catch (e) { /* no storage */ }
    if (view !== '2d' && !P.open) setOpen(true);
    applyView();
    if (P.onView) P.onView(view);
    if (P.onLayout) P.onLayout(view);
  }
  function applyView() {
    const on = P.visible && P.mapsTab !== false;
    applySplit();
    document.body.classList.toggle('xw-studio', on && P.open && P.view !== '2d');
    document.body.classList.toggle('xw-view-split', on && P.view === 'split');
    document.body.classList.toggle('xw-view-3d', on && P.view === '3d');
    for (const [k, b] of Object.entries(viewBtns)) b.classList.toggle('is-on', k === P.view);
    viewBox.hidden = P.mapsTab === false;
  }

  function setOpen(on) {
    P.open = on;
    toggleBtn.textContent = on ? 'Close 3D preview' : 'Open 3D preview';
    body.hidden = !on;
    ctrls.hidden = !on;
    editSlot.hidden = !on || !editSlot.firstChild || !!P.editOff;
    layerBox.hidden = !on;
    if (on && !P.frame) {
      const f = document.createElement('iframe');
      f.className = 'xw-3d-frame';
      f.title = '3D map preview';
      f.src = new URL('../play.html?xwpreview=1', import.meta.url).href;
      P.frame = f;
      P.ready = false;
      P.dirty = true;
      body.append(f);
      setStatus('Starting the engine and reading your disc ...', false, 'hold');
      applyView();
    } else if (!on && P.frame) {
      // Closing frees the frame's engine and its copy of the disc.
      P.frame.remove();
      P.frame = null;
      P.ready = false;
      P.sent = null;
      setStatus('');
      // No picture to lay out beside the painter.
      if (P.view !== '2d') { P.view = '2d'; applyView(); }
    }
    applyView();
    setMode(P.mode);
  }

  function pack() { return env.pack(); }

  /* The pack's stripped JSON (strippedJson) as it is now: made once an
   * edit (the pack serialised is the cost; a decision and its flush ask
   * for the same text). */
  function stripped() {
    if (P.stripAt && P.stripAt.edits === P.edits && P.stripAt.map === P.map && P.stripAt.pack === pack()) return P.stripAt.json;
    const json = strippedJson(pack(), P.map);
    P.stripAt = { edits: P.edits, map: P.map, pack: pack(), json };
    return json;
  }

  /* Export and post the pack to the frame now. */
  function flush() {
    clearTimeout(P.timer);
    if (!P.open || !P.ready || !P.visible || !P.map || !pack()) return;
    if (!pack().maps || !pack().maps[P.map]) { setStatus(`No map ${P.map} in the pack.`, true); return; }
    // A forced rebuild (Reload, a move the frame refused) stays forced
    // until it is sent.
    if (!P.forceFull && moveObjects()) return;
    /* The painter's preview of the edit is still to come: let its draft
     * reach the frame first (the rebuild then holds the thread for a
     * while), once. */
    if (P.sent && P.pvNowEdits !== P.edits && !P.deferred) {
      P.deferred = true;
      P.timer = setTimeout(flush, 250);
      return;
    }
    P.deferred = false;
    /* The bytes are built off this thread when the page can
     * (`exportBundleAsync`, the validation worker); a build already being
     * made takes this edit as its next. */
    if (P.building) { P.dirty = true; return; }
    const seq = (P.buildSeq || 0) + 1;
    P.buildSeq = seq;
    P.building = true;
    const tb = performance.now();
    const fail = (e) => {
      P.building = false;
      setStatus(`Showing the last good build: the pack has errors (${String(e && e.message ? e.message : e).split('\n')[0]}).`, true);
    };
    let made;
    try {
      made = env.exportBundleAsync ? env.exportBundleAsync() : Promise.resolve(env.exportBundle());
    } catch (e) { fail(e); return; }
    // What the build is measured against: the pack as it is now.
    const snap = { map: P.map, edits: P.edits, stripped: stripped(), shown: objectPoses(pack().maps[P.map]) };
    made.then((bytes) => {
      if (seq !== P.buildSeq) return;
      P.building = false;
      P.bundleMs = Math.round(performance.now() - tb);
      sendBuild(bytes, snap);
      // An edit that came while the bytes were made.
      if (P.dirty) schedule();
    }, fail);
  }

  function sendBuild(bytes, snap) {
    if (!P.open || !P.ready || !P.frame) return;
    const first = !P.sent;
    P.forceFull = false;
    P.sent = { map: snap.map };
    // The tiles the draft compares against are this build's: the latest
    // preview when no edit came after it, else the next one to arrive.
    P.builtEdits = snap.edits;
    P.pvBuilt = P.pvNow && P.pvNowEdits === snap.edits ? P.pvNow : null;
    // The draft sent with the overlay after the build: nothing beyond it.
    const def = pack().maps[snap.map];
    if (P.pvBuilt && def) P.defs = draftDefs(def, P.pvNow, P.pvBuilt);
    // An edit after the snapshot the bytes were made from stays dirty.
    P.dirty = P.edits !== snap.edits;
    P.t0 = performance.now();
    if (first) setStatus(`Entering ${snap.map} ...`, false, 'hold');
    else setStatus(`Rebuilding ${snap.map} ...`, false, 'dev');
    P.lastKind = first ? 'xw-load' : 'xw-reload';
    // What an object-only edit is measured against: this build.
    P.sentStripped = snap.stripped;
    P.shown = snap.shown;
    P.stale = false;
    post({ type: P.lastKind, id: pack().manifest.id, map: snap.map, spawn: P.spawn, bytes }, [bytes.buffer]);
  }

  /* An edit since the last build that only moved or turned props, NPCs,
   * spawns and lamps of the open map goes to the frame as moves
   * (`xw-move`): the drawn objects move at once, the engine's copy of each
   * (a prop's closed tile and standable top, a lamp's light, a spawn)
   * follows, and nothing is rebuilt. A frame whose bundle cannot move the
   * engine's copy of a prop says so (`collision: false`) and the Player
   * camera / Walk rebuild first. True when the edit was handled here (or
   * there was none). */
  function moveObjects() {
    if (P.building || !P.sent || P.sent.map !== P.map || !P.shown || !P.sentStripped) return false;
    const def = pack().maps[P.map];
    if (stripped() !== P.sentStripped) return false;
    const now = objectPoses(def);
    for (const k of ['props', 'npcs', 'spawns', 'lamps']) if (now[k].length !== P.shown[k].length) return false;
    const mv = objectMoves(P.shown, now, P.grid);
    if (!mv) return false;
    P.dirty = false;
    if (!mv.props.length && !mv.npcs.length && !mv.spawns.length && !mv.lamps.length) return true;
    P.shown = now;
    P.moveSeq = (P.moveSeq || 0) + 1;
    P.moveT0 = performance.now();
    post({ type: 'xw-move', seq: P.moveSeq, props: mv.props, npcs: mv.npcs, spawns: mv.spawns, lamps: mv.lamps });
    return true;
  }

  /* Rebuild now if props were moved in place since the last build: the
   * player is about to walk among them. */
  function rebuildIfStale() {
    if (!P.stale) return;
    P.forceFull = true;
    P.dirty = true;
    flush();
  }

  /* The rebuild runs on this page's main thread, so a map that takes a
   * while to build waits for a longer pause in the editing: the delay grows
   * with the last build's time. With Auto off only Reload rebuilds. */
  function schedule() {
    clearTimeout(P.timer);
    // Which wait applies is decided once a burst of edits pauses (a drag
    // or a stroke edits every frame), not per edit: telling an object-only
    // edit apart serialises the pack.
    P.timer = setTimeout(decide, DECIDE_MS);
  }

  function decide() {
    // An object-only edit costs nothing to show: send it at once, Auto or not.
    const def = P.map && pack() && pack().maps && pack().maps[P.map];
    if (P.sent && P.shown && def && P.sent.map === P.map && stripped() === P.sentStripped) {
      flush();
      return;
    }
    if (!P.auto && P.sent) return;
    // The last hot reload's time (a first entry builds more and says
    // nothing about the next edit).
    const last = P.reloadMs || 0;
    P.timer = setTimeout(flush, Math.max(RELOAD_MS, Math.min(5000, last * 1.5)) - DECIDE_MS);
  }

  function sendOverlay() {
    post({ type: 'xw-overlay', layers: P.layers, select: P.select, path: P.path, marker: P.marker, grid: P.grid, defs: P.defs });
  }

  window.addEventListener('message', (ev) => {
    if (!P.frame || ev.source !== P.frame.contentWindow || ev.origin !== location.origin) return;
    const m = ev.data || {};
    if (m.type === 'xw-ready') {
      P.ready = true;
      P.sent = null;
      post({ type: 'xw-arrival', on: P.arrival });
      sendBackdrop();
      syncRun();
      sendEdit();
      flush();
    } else if (m.type === 'xw-loaded') {
      const ms = Math.round(performance.now() - (P.t0 || performance.now()));
      P.builds = (P.builds || 0) + 1;
      if (m.ok && P.lastKind === 'xw-reload') P.reloadMs = ms;
      P.last = { ok: m.ok, ms, engineMs: m.engineMs, map: m.map, n: P.builds };
      if (m.busy) {
        // A fight or a screen owns the frame: try again shortly.
        setStatus('The preview is in a fight or a screen: the edit goes in once it is back on the field.');
        P.dirty = true;
        clearTimeout(P.timer);
        P.timer = setTimeout(flush, 1500);
      } else if (!m.ok) {
        setStatus(`The engine refused the pack: ${(m.errors || []).slice(0, 2).join(' | ')}`, true);
        if (!m.map) P.sent = null;
      } else {
        setStatus(`${m.map}: built in ${ms} ms${m.engineMs != null ? ` (engine ${m.engineMs} ms)` : ''}.`, false, 'dev');
      }
      sendOverlay();
      sendEdit();
      post({ type: 'xw-camera', mode: P.mode, painter: P.painter });
      syncRun();
      if (m.ok && P.pending) sendPending();
    } else if (m.type === 'xw-moved') {
      P.lastMove = { ok: !!m.ok, ms: Math.round(performance.now() - (P.moveT0 || performance.now())), props: m.props || 0, npcs: m.npcs || 0, spawns: m.spawns || 0, lamps: m.lamps || 0, collision: !!m.collision, seq: m.seq };
      if (!m.ok) {
        // The frame could not place them: rebuild.
        P.forceFull = true;
        P.dirty = true;
        flush();
      } else {
        // A bundle without the engine's prop move leaves a moved prop's
        // collision where the build put it: rebuild before the player walks.
        if (m.props && !m.collision) P.stale = true;
        const n = (m.props || 0) + (m.npcs || 0) + (m.spawns || 0) + (m.lamps || 0);
        setStatus(`${P.map}: moved ${n} object(s) in place in ${P.lastMove.ms} ms, no rebuild.${P.stale ? ' A moved prop\'s collision updates on the next rebuild (Reload, Player or Walk).' : (m.props ? ' Its collision moved with it.' : '')}`, false, 'dev');
      }
    } else if (m.type === 'xw-collision') {
      P.collision = m;
      if (P.onCollision) P.onCollision(m);
    } else if (m.type === 'xw-pick') {
      P.select = { tile: m.tile };
      if (P.onPick) P.onPick(m.tile[0], m.tile[1]);
    } else if (m.type === 'xw-action') {
      const what = { cutscene: `Cutscene ${m.id}`, fight: `Fight ${m.id}`, cards: 'The card binder', mode: `Mode ${m.id}` }[m.what] || m.what;
      if (m.what === 'mode' && !m.ok) { setStatus(`Mode ${m.id} did not start in the preview: ${m.why || 'the field is busy'}`, true); return; }
      setStatus(m.ok ? `${what}: running in the preview (field camera; Orbit to look around).${m.what === 'cards' ? ` ${m.added} card(s) put in its collection.` : ''}`
        : `${what} did not start in the preview (another scene or fight may own it).`, !m.ok);
    } else if (m.type === 'xw-stats') {
      P.stats = m;
      paintStatus();
    } else if (m.type === 'xw-key' && m.key === 'topdown') {
      P.top = !!m.on;
      topBtn.classList.toggle('is-on', P.top);
    } else if (/^xw-(select|transform|sculpt|paint|key|place)$/.test(m.type)) {
      // The frame's editing layer answered (expansion-workbench-gizmos.js).
      if (P.onEdit) P.onEdit(m);
    }
  });

  /* The 3D editor's state for the frame (`xw-edit`): kept whole, so a new
   * frame or a rebuilt map gets all of it again. */
  function sendEdit() {
    if (P.edit) post(Object.assign({ type: 'xw-edit' }, P.edit, P.editOff ? { mode: 'view' } : {}));
  }

  /* An action for the frame's engine (a cutscene, a fight, the binder),
   * sent once the map it runs on is built. With no map chosen yet, the
   * pack's first map stands in. */
  function act(msg) {
    if (!P.map && pack() && pack().maps) {
      const first = Object.keys(pack().maps)[0];
      if (first) { P.map = first; P.dirty = true; schedule(); }
    }
    P.pending = msg;
    if (P.ready && P.sent && !P.dirty) sendPending();
  }

  function sendPending() {
    const msg = P.pending;
    P.pending = null;
    setMode('player', true);
    post(msg);
  }

  function statsText() {
    const s = P.stats;
    return s ? ` Frame ${s.frameMs.toFixed(1)} ms (worst ${s.worstMs.toFixed(1)}), ${Math.round(s.fps)} fps.` : '';
  }

  /* --- Play here --- */
  function playHere(map) {
    let bytes;
    try { bytes = env.exportBundle(); } catch (e) { return Promise.reject(e); }
    const id = pack().manifest.id;
    return new Promise((resolve) => {
      const nonce = Math.random().toString(36).slice(2);
      let done = false;
      const finish = async (answer) => {
        if (done) return;
        done = true;
        if (channel) channel.removeEventListener('message', onMsg);
        if (answer) { resolve({ reloaded: true, answer }); return; }
        // No play tab on this pack answered: hand off through the slot.
        await env.storePlay(bytes);
        window.open(env.playUrl(map), 'legaia-play');
        resolve({ reloaded: false });
      };
      const onMsg = (ev) => {
        const m = ev.data || {};
        if (m.nonce === nonce && m.type === 'reloaded') finish(m);
      };
      if (!channel) { finish(null); return; }
      channel.addEventListener('message', onMsg);
      // Structured clone copies the bytes; keep ours for the slot fallback.
      channel.postMessage({ type: 'reload', nonce, id, map, bytes });
      setTimeout(() => finish(null), 700);
    });
  }

  /* Live: every edit goes to an open play tab too. */
  function pushLive() {
    clearTimeout(P.liveTimer);
    if (!P.live || !channel || !pack()) return;
    P.liveTimer = setTimeout(() => {
      let made;
      try { made = env.exportBundleAsync ? env.exportBundleAsync() : Promise.resolve(env.exportBundle()); } catch (e) { return; }
      made.then((bytes) => channel.postMessage({ type: 'reload', nonce: 'live', id: pack().manifest.id, map: P.map, bytes }), () => {});
    }, RELOAD_MS * 2);
  }

  /* Run the frame's loop only while the dock is on screen and the painter
   * is not mid-stroke: the frame shares this page's main thread. */
  let onScreen = true;
  let held = false;
  const syncRun = () => post({ type: 'xw-run', on: P.visible && P.open && onScreen && !held });
  if (typeof IntersectionObserver === 'function') {
    new IntersectionObserver((es) => {
      onScreen = es.some((e) => e.isIntersecting);
      syncRun();
    }).observe(body);
  }

  return {
    el,
    /* The painter holds the frame still through a stroke. */
    hold(on) {
      if (held === !!on) return;
      held = !!on;
      syncRun();
    },
    /* The tab now showing: the preview runs on the Maps and Cutscenes tabs. */
    show(on) {
      P.visible = !!on;
      el.hidden = !on;
      applyView();
      syncRun();
      if (P.visible && P.dirty) schedule();
    },
    /* The map the editor is on (and the spawn to enter it at). */
    setMap(map, spawn) {
      if (map === P.map) return;
      P.map = map;
      P.spawn = spawn || '';
      P.select = null;
      P.path = null;
      P.marker = null;
      P.grid = null;
      P.defs = null;
      P.overlayKey = '';
      P.lastPv = null;
      P.pvNow = null;
      P.pvBuilt = null;
      P.dirty = true;
      schedule();
    },
    /* The painter's latest \`map_preview\` of the map it is on: the grid and
     * the draft the frame draws until the next rebuild lands. Sent only
     * when it changed. */
    setMapPreview(map, pv) {
      // A new preview object is a new computation (the painter makes one a
      // moment after each edit); the same one has been sent.
      if (!map || !pv || pv.error || pv === P.lastPv) return;
      P.lastPv = pv;
      const grid = gridCorners(map, pv);
      P.pvNow = pv;
      P.pvNowEdits = P.edits;
      // The first preview after a build with no edit since it describes
      // exactly the pack that build was sent.
      if (!P.pvBuilt && P.builtEdits === P.edits) P.pvBuilt = pv;
      const defs = draftDefs(map, pv, P.pvBuilt);
      const key = JSON.stringify([grid, defs]);
      if (key === P.overlayKey) return;
      P.overlayKey = key;
      P.grid = grid;
      P.defs = defs;
      post({ type: 'xw-overlay', grid, defs });
    },
    /* The pack was edited. */
    changed() {
      P.edits = (P.edits || 0) + 1;
      P.dirty = true;
      if (P.visible) schedule();
      pushLive();
    },
    /* The painter's selection: `{ tile }`, `{ rect }` or null. */
    select(sel) {
      const k = JSON.stringify(sel || null);
      if (k === JSON.stringify(P.select || null)) return;
      P.select = sel || null;
      sendOverlay();
    },
    onPick(fn) { P.onPick = fn; },
    /* The engine's collision reading of the last build (or in-place move):
     * `{ map, width, height, blocked, walls }`, a quarter mask a tile. */
    onCollision(fn) { P.onCollision = fn; if (fn && P.collision) fn(P.collision); },
    /* A cutscene's camera path on the map, and the scrub point. */
    setPath(path, marker) {
      const first = !P.path && path;
      P.path = path || null;
      P.marker = marker || null;
      sendOverlay();
      // A path is read from above.
      if (first && P.mode !== 'orbit') setMode('orbit');
    },
    /* Play cutscene `id` in the frame, once the map it plays on is built. */
    playCutscene(id) { act({ type: 'xw-cutscene', id }); },
    /* Start a pack fight (`kind` "formation" or "boss") in the frame. */
    fight(kind, id) { act({ type: 'xw-fight', kind, id }); },
    /* Start mode instance `id` in the frame, on pack map `map` (built
     * first) when it is played on one. */
    mode(id, map) {
      if (map) this.setMap(map, '');
      act({ type: 'xw-mode', id });
    },
    /* Every pack card into the frame's collection, and its binder open. */
    cards() { act({ type: 'xw-cards' }); },
    /* The 3D editor: its state for the frame (merged), the handler of the
     * frame's answers, and its bar. */
    edit(msg) {
      P.edit = Object.assign(P.edit || {}, msg);
      if (P.ready) post(Object.assign({ type: 'xw-edit' }, msg, P.editOff ? { mode: 'view' } : {}));
    },
    /* The real model of the piece being placed, for the frame's ghost. */
    ghost(msg) {
      if (P.ready) post(Object.assign({ type: 'xw-ghost' }, msg));
    },
    /* Off the Maps tab the 3D editor stands down: the frame drops to the
     * view mode and the edit bar hides; back on the tab both come back as
     * they were. */
    suspendEdit(off) {
      off = !!off;
      // The view switch belongs to the Maps tab as the editor does.
      P.mapsTab = !off;
      applyView();
      if (!off && P.visible && P.view !== '2d' && !P.open) setOpen(true);
      if (P.editOff === off) return;
      P.editOff = off;
      editSlot.hidden = off || !editSlot.firstChild || !P.open;
      if (P.ready) sendEdit();
    },
    onEdit(fn) { P.onEdit = fn; },
    /* The view switch moved (the 3D editor picks its mode by it). */
    onView(fn) { P.onView = fn; },
    setEditBar(bar) {
      if (bar && bar.parentNode !== editSlot) { editSlot.innerHTML = ''; editSlot.append(bar); }
      editSlot.hidden = !bar || !P.open || !!P.editOff;
    },
    /* The panels over the picture (the editor's). */
    setFloat(node) {
      if (node.parentNode !== float) { float.innerHTML = ''; float.append(node); }
    },
    /* The layout moved (the page's head follows). */
    onLayout(fn) { P.onLayout = fn; },
    /* The Split view's drawer, open or put away. */
    setDrawer: (on) => setView(on ? 'split' : '3d'),
    drawerOpen: () => P.view === 'split',
    view: () => P.view,
    isOpen: () => P.open,
    /* A line on the preview's status (the editor's notes). */
    note: (text, bad) => setStatus(text, bad),
    /* Test play on / off (the party on the map, the game's keys), and
     * whether it is running. */
    testPlay: (on) => testPlay(on === undefined ? P.mode !== 'player' : !!on),
    testing: () => P.mode === 'player',
    /* The editor camera straight down (`on`) or tilted back. */
    topDown(on) {
      P.top = !!on;
      topBtn.classList.toggle('is-on', P.top);
      if (P.mode !== 'orbit') setMode('orbit');
      post({ type: 'xw-camera', mode: 'orbit', painter: P.painter, top: P.top });
    },
    onMode(fn) { P.onMode = fn; },
    /* The camera, `orbit` or `player` (the shell's Test-play mode). */
    setCamera: (mode) => {
      if (mode === 'player' && !P.open) setOpen(true);
      if (mode === 'player') testPlay(true);
      else if (mode === 'orbit') setMode('orbit');
    },
    open: () => { if (!P.open) setOpen(true); },
    playHere,
    setLive(on) { P.live = !!on; pushLive(); },
    /* Headless-verification hook. */
    setView,
    setSplit: (f) => setSplit(f, true),
    state: () => ({ split, lastMove: P.lastMove, bundleMs: P.bundleMs, building: !!P.building, stale: !!P.stale, view: P.view, open: P.open, ready: P.ready, map: P.map, last: P.last, stats: P.stats, select: P.select, dirty: P.dirty, mode: P.mode, painter: P.painter, arrival: P.arrival, status: status.textContent }),
  };
}
