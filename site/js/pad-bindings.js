/* Keyboard -> PSX pad bits, for every page on this site.
 *
 * Deliberately not a table. There used to be three of them - play-app.js,
 * play.html's title loop and minigames.html - and none agreed with the
 * engine's own layout or with each other: `X` was Square in the native window
 * and Circle on the pages, `S` was Circle natively and Down here, and
 * minigames.html bound `A`/`S`/`D` to the three face buttons the engine binds
 * to Left / Down / Right. None of that shows up in a diff, because no file
 * held two of the columns.
 *
 * So every page reads the binding table out of the engine instead
 * (`legaia_engine_core::input::Mapping::web_default` through
 * `pad_bindings_json`). A rebind now lands on every host at once and a
 * disagreement can no longer be written down.
 *
 * `web_default`, not `default`: the play page walks on WASD as well as the
 * arrows, and the desktop layout spends A / S / W on Triangle / Circle / R1.
 * One `HashMap<key, button>` cannot hold both `S -> Down` and `S -> Circle`,
 * so the engine carries two *named layouts* rather than one layout plus an
 * override table here - which is the whole point, since an override table in
 * a page file is the thing that drifted last time. The face buttons live on
 * Z / X / C / V and the shoulders on Q / E.
 *
 * Loaded before play-app.js and before each page's own script. */
(function () {
  'use strict';

  let PAD = null;
  let PAD_BTN = null;
  let SWALLOW = new Set(['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Space']);

  /* Bindings of last resort, used only against a cached wasm bundle that
   * predates the export. Deliberately a bare minimum - walk, confirm, cancel,
   * menu - because this is a degraded mode, not a second layout to maintain.
   * The console line is the point: a silent fallback here is how the tables
   * diverged the first time. */
  const PAD_STALE_FALLBACK = {
    ArrowUp: 0x0010, ArrowRight: 0x0020, ArrowDown: 0x0040, ArrowLeft: 0x0080,
    KeyZ: 0x4000, KeyX: 0x2000, Enter: 0x0008,
  };

  /* Adopt the engine's binding table. `src` is a LegaiaRuntime, a
   * LegaiaMinigames or the wasm module namespace - all three export the same
   * two calls. Idempotent. */
  function adoptPadBindings(src) {
    if (PAD) return PAD;
    let table = null, buttons = null;
    try {
      if (src && typeof src.pad_bindings_json === 'function') {
        table = JSON.parse(src.pad_bindings_json());
        buttons = JSON.parse(src.pad_buttons_json());
      }
    } catch (e) { table = null; }
    if (!table || !Object.keys(table).length) {
      console.warn('[legaia] engine pad bindings unavailable (stale wasm bundle?) - '
        + 'falling back to arrows + Z/X/Enter only. Rebuild site/wasm/.');
      table = PAD_STALE_FALLBACK;
      buttons = null;
    }
    PAD = table;
    PAD_BTN = buttons;
    /* Keys the canvas swallows so the page doesn't scroll under the player. */
    SWALLOW = new Set(Object.keys(PAD).concat(['ArrowUp', 'ArrowDown',
      'ArrowLeft', 'ArrowRight', 'Space']));
    return PAD;
  }

  /* PSX digital-pad word layout. Hardware, not a binding choice: these bits
   * are the same in the engine, the recomp and the console, and the runtime's
   * `set_pad` doc lists them. Used only to answer `legaiaPadButton` against a
   * stale bundle that has no button table to serve. */
  const PAD_BITS = {
    Select: 0x0001, L3: 0x0002, R3: 0x0004, Start: 0x0008,
    Up: 0x0010, Right: 0x0020, Down: 0x0040, Left: 0x0080,
    L2: 0x0100, R2: 0x0200, L1: 0x0400, R1: 0x0800,
    Triangle: 0x1000, Circle: 0x2000, Cross: 0x4000, Square: 0x8000,
  };

  /* Fold a set of key codes into a pad word. */
  function padMaskOf(keys) {
    let mask = 0;
    for (const k of keys) mask |= (PAD && PAD[k]) || 0;
    return mask;
  }

  window.legaiaAdoptPadBindings = adoptPadBindings;
  window.legaiaPadTable = () => PAD;
  window.legaiaPadMaskOf = padMaskOf;
  window.legaiaPadButton = (name) => (PAD_BTN && PAD_BTN[name]) || PAD_BITS[name] || 0;
  window.legaiaPadSwallows = (code) => SWALLOW.has(code);
  /* The button a `KeyboardEvent.code` is bound to, or `''`. The shape a page
   * whose controls are named buttons ("Circle casts") wants: it asks for the
   * button, never for the key. */
  window.legaiaPadButtonOf = (code) => {
    const bit = (PAD && PAD[code]) || 0;
    if (!bit) return '';
    for (const name of Object.keys(PAD_BTN || PAD_BITS)) {
      if (((PAD_BTN && PAD_BTN[name]) || PAD_BITS[name]) === bit) return name;
    }
    return '';
  };
  /* The `KeyboardEvent.code`s bound to a named button, for a page that has to
   * *print* its controls ("Square = V"). Derived, never typed. */
  window.legaiaPadKeysFor = (name) => {
    const bit = window.legaiaPadButton(name);
    if (!bit || !PAD) return [];
    return Object.keys(PAD).filter((code) => PAD[code] === bit);
  };
}());
