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
 * (`legaia_engine_system::input::Mapping::web_default` through
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
 * That layout is now the DEFAULT rather than the whole story: the pause
 * menu's Options > Key Config screen rebinds keys, and the engine persists the
 * edited table to localStorage. A page that folded this table into `PAD` at
 * load therefore has to re-read it - `legaiaSyncPadBindings(rt)` does that off
 * the engine's own revision counter. Still no table in this file.
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
   * two calls. Idempotent unless `force` is set. */
  function adoptPadBindings(src, force) {
    if (PAD && !force) return PAD;
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
    adoptGamepadLayout(src);
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

  /* Fold a set of key codes into a pad word. A `Gamepad:<Button>` entry
   * (see `legaiaPollGamepad`) names a pad button directly. */
  function padMaskOf(keys) {
    let mask = 0;
    for (const k of keys) {
      if (k.startsWith('Gamepad:')) mask |= window.legaiaPadButton(k.slice(8));
      else mask |= (PAD && PAD[k]) || 0;
    }
    return mask;
  }

  /* Gamepad API, `standard` mapping -> the PSX pad. A gamepad is a
   * *controller*, not a key layout, so it binds buttons to buttons by
   * physical position and the Key Config table (which maps keys) never
   * reaches it.
   *
   * Not a table either: the engine serves the layout
   * (`legaia_engine_system::gamepad` through `gamepad_layout_json`), and the
   * native window folds gilrs through the same one, so the button order,
   * stick dead zone and trigger threshold cannot drift between hosts. Against
   * a bundle that predates the export no controller is read - a degraded
   * mode, said so on the console, rather than a second layout here. */
  let GP_LAYOUT = null;
  function adoptGamepadLayout(src) {
    let layout = null;
    try {
      if (src && typeof src.gamepad_layout_json === 'function') {
        layout = JSON.parse(src.gamepad_layout_json());
      }
    } catch (e) { layout = null; }
    if (!layout || !Array.isArray(layout.buttons) || !layout.buttons.length) {
      if (src) console.warn('[legaia] engine gamepad layout unavailable (stale wasm bundle?) - '
        + 'controllers are not read. Rebuild site/wasm/.');
      return;
    }
    GP_LAYOUT = layout;
  }
  let gamepadHeld = new Set();

  /* The pad buttons every connected standard-mapped gamepad holds now. */
  function gamepadButtons() {
    const out = new Set();
    if (!GP_LAYOUT) return out;
    const names = GP_LAYOUT.buttons;
    const dead = GP_LAYOUT.stick_dead_zone;
    const press = GP_LAYOUT.press_threshold;
    let pads = [];
    try { pads = (navigator.getGamepads && navigator.getGamepads()) || []; } catch (e) { pads = []; }
    for (const gp of pads) {
      if (!gp || !gp.connected) continue;
      gp.buttons.forEach((b, i) => {
        const name = names[i];
        if (name && (b.pressed || b.value > press)) out.add(name);
      });
      const [x, y] = [gp.axes[0] || 0, gp.axes[1] || 0];
      if (x <= -dead) out.add('Left');
      if (x >= dead) out.add('Right');
      if (y <= -dead) out.add('Up');
      if (y >= dead) out.add('Down');
    }
    return out;
  }

  /* One poll of the controllers: the button names held now, and those newly
   * held / newly released since the last poll (`StandardGamepad::update`).
   * The edge half both helpers below share. Call once per display frame. */
  function pollGamepadEdges() {
    const now = gamepadButtons();
    const pressed = [], released = [];
    for (const name of now) if (!gamepadHeld.has(name)) pressed.push(name);
    for (const name of gamepadHeld) if (!now.has(name)) released.push(name);
    gamepadHeld = now;
    return { now, pressed, released };
  }
  /* For a page whose controls are named buttons (the minigames page): it
   * dispatches each edge through the same handler a key's button takes. */
  window.legaiaGamepadEdges = () => {
    const { pressed, released } = pollGamepadEdges();
    return { pressed, released };
  };

  /* Merge this frame's gamepad state into a page's keyboard `held` / `pulse`
   * sets as `Gamepad:<Button>` entries: a newly held button lands in both
   * (a press edge, like a keydown), a released one leaves `held`. Returns
   * `true` when anything changed, so the caller re-packs its pad word. Call
   * once per display frame, before the frame reads `pulse`. */
  window.legaiaPollGamepad = (held, pulse) => {
    if (!GP_LAYOUT) return false;
    const { now, pressed, released } = pollGamepadEdges();
    let changed = false;
    for (const name of now) {
      /* Re-asserted every frame, not only on the edge: a page clears its
       * `held` set on a modal hand-off, and a key's auto-repeat refills it
       * where a button held through the hand-off would otherwise read as
       * released until pressed again. */
      const key = 'Gamepad:' + name;
      if (!held.has(key)) { held.add(key); changed = true; }
    }
    if (pulse) for (const name of pressed) { pulse.add('Gamepad:' + name); changed = true; }
    for (const name of released) { held.delete('Gamepad:' + name); changed = true; }
    return changed;
  };
  /* A page that clears its `held` set (focus loss) forgets the pad too, so
   * a still-held button re-enters as a fresh press rather than a stuck one
   * (`StandardGamepad::reset`, the native window's focus rule). */
  window.legaiaResetGamepad = () => { gamepadHeld = new Set(); };

  window.legaiaAdoptPadBindings = adoptPadBindings;
  /* Re-read the engine's table after the options screen's Key Config screen
   * rebound something. The table above is folded into `PAD` once at load, so
   * nothing about the engine serving new bindings would reach a running page;
   * `pad_bindings_revision` is the engine's own change counter and this is the
   * only thing that acts on it.
   *
   * Cheap enough to call once a frame: it compares one integer and returns. */
  let padRev = -1;
  window.legaiaSyncPadBindings = (src) => {
    if (!src || typeof src.pad_bindings_revision !== 'function') return false;
    let rev;
    try { rev = src.pad_bindings_revision() | 0; } catch (e) { return false; }
    if (rev === padRev) return false;
    const first = padRev < 0;
    padRev = rev;
    if (first) return false;       /* the load-time read is not a rebind */
    adoptPadBindings(src, true);
    return true;
  };
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
