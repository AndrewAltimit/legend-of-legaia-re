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
    /* The controller fold's sources follow the newest holder even when the
     * table is already adopted: the title loop adopts the module namespace,
     * the runtime that arrives later is the one with the dead zone and the
     * Key Config capture. */
    adoptPadSources(src);
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

  /* Gamepad API -> the PSX pad. Not a table here either: each pad's button
   * values and axes go to the engine's fold (`gamepad_fold`,
   * `GamepadMapping::fold_pad`), the one the native window runs gilrs
   * through, over the player's controller table (rebound in Options > Key
   * Config, stored with the keys) at the player's stick dead zone. A pad that
   * does not report the `standard` mapping folds through the table's raw half,
   * with its d-pad read off a hat the engine finds from the axes' resting
   * values (taken the first time the pad is seen).
   *
   * Against a bundle that predates `gamepad_fold` the page falls back to the
   * fixed layout (`gamepad_layout_json`), standard order, every pad; against
   * one that predates that too no controller is read - a degraded mode, said
   * so on the console, rather than a second layout here. */
  let GP_LAYOUT = null;
  /* The object whose `gamepad_fold` (and `play_menu_pad_button`, for a Key
   * Config capture) this page calls: the runtime, or the module namespace. */
  let FOLD_SRC = null;
  let CAPTURE_SRC = null;
  function adoptPadSources(src) {
    if (!src) return;
    if (typeof src.play_menu_pad_button === 'function') CAPTURE_SRC = src;
    /* A runtime outranks the module namespace: it also serves the dead zone. */
    if (typeof src.gamepad_fold === 'function'
        && (!FOLD_SRC || typeof src.stick_dead_zone === 'function')) FOLD_SRC = src;
  }
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

  /* Buttons an on-screen control holds (the play page's touch pad,
   * touch-pad.js), by pad button name. They join the controller poll below
   * rather than keeping a path of their own: every page already merges that
   * poll into its pad word, so a touch press is a controller press to every
   * reader - the field tick, the menus, the title card, the minigames page's
   * named-button handlers - and needs no call site of its own. */
  const virtualHeld = new Set();
  window.legaiaVirtualPad = {
    press(name) { virtualHeld.add(name); },
    release(name) { virtualHeld.delete(name); },
    releaseAll() { virtualHeld.clear(); },
    held() { return Array.from(virtualHeld); },
  };

  /* Per connected pad (keyed by index and id): the axes as first seen, the
   * raw buttons held at the last poll, and whether it reports `standard`. */
  const padState = new Map();
  const padKey = (gp) => gp.index + '|' + gp.id;
  /* Controller presses since the last flush, for Key Config. */
  let captures = [];
  /* Which device the prompts follow: the engine decides the label
   * (`pad_prompt_label`), the page only notices the switch to re-label. */
  let lastDevice = 'keyboard';
  function noteDevice(dev) {
    if (dev === lastDevice) return;
    lastDevice = dev;
    try { window.dispatchEvent(new CustomEvent('legaia-input-device', { detail: dev })); } catch (e) {}
  }

  function bitNames(mask, out) {
    const table = PAD_BTN || PAD_BITS;
    for (const name of Object.keys(table)) if (mask & table[name]) out.add(name);
  }

  /* The pad buttons every connected gamepad holds now, through the engine's
   * fold, and the on-screen control's. */
  function gamepadButtons() {
    const out = new Set(virtualHeld);
    if (FOLD_SRC) {
      let pads = [];
      try { pads = (navigator.getGamepads && navigator.getGamepads()) || []; } catch (e) { pads = []; }
      let dz = 50;
      try { if (typeof FOLD_SRC.stick_dead_zone === 'function') dz = FOLD_SRC.stick_dead_zone() | 0; } catch (e) {}
      const seen = new Set();
      for (const gp of pads) {
        if (!gp || !gp.connected) continue;
        const key = padKey(gp);
        seen.add(key);
        let st = padState.get(key);
        if (!st) {
          st = { rest: Array.from(gp.axes), held: new Set(), standard: gp.mapping === 'standard' };
          padState.set(key, st);
        }
        const vals = gp.buttons.map((b) => (b.pressed ? 1 : (b.value || 0)));
        const held = new Set();
        vals.forEach((v, i) => { if (v > 0.5) held.add(i); });
        /* A button held through a rebind stays out until let go
         * (`swallowHeldPads`). */
        if (st.swallow) {
          for (const i of Array.from(st.swallow)) {
            if (held.has(i)) vals[i] = 0; else st.swallow.delete(i);
          }
        }
        for (const i of held) {
          if (!st.held.has(i)) captures.push({ standard: st.standard, index: i, id: gp.id });
        }
        st.held = held;
        let mask = 0;
        try {
          mask = FOLD_SRC.gamepad_fold(st.standard, vals, Array.from(gp.axes), st.rest, gp.id, dz);
        } catch (e) { mask = 0; }
        if (mask) noteDevice('pad');
        bitNames(mask, out);
      }
      for (const key of Array.from(padState.keys())) if (!seen.has(key)) padState.delete(key);
      return out;
    }
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
  /* A rebind just landed: every controller button held now stays out of
   * the fold until it is let go. The button that made a Key Config bind is
   * still down, and through the new table it is another PSX button - read
   * as a fresh press of it, a bind onto Cross re-opened the row's wait
   * (the native window's `GamepadHost::swallow_held`). */
  function swallowHeldPads() {
    for (const st of padState.values()) st.swallow = new Set(st.held);
  }
  /* Hand this poll's controller presses to Key Config
   * (`play_menu_pad_button`, which only a waiting rebind row consumes). */
  function flushCaptures() {
    const list = captures;
    captures = [];
    if (!CAPTURE_SRC) return;
    for (const c of list) {
      try { CAPTURE_SRC.play_menu_pad_button(c.standard, c.index, c.id); } catch (e) { return; }
    }
  }
  /* For a page whose controls are named buttons (the minigames page): it
   * dispatches each edge through the same handler a key's button takes. */
  window.legaiaGamepadEdges = () => {
    const { pressed, released } = pollGamepadEdges();
    captures = [];
    return { pressed, released };
  };

  /* Merge this frame's gamepad state into a page's keyboard `held` / `pulse`
   * sets as `Gamepad:<Button>` entries: a newly held button lands in both
   * (a press edge, like a keydown), a released one leaves `held`. Returns
   * `true` when anything changed, so the caller re-packs its pad word. Call
   * once per display frame, before the frame reads `pulse`. */
  window.legaiaPollGamepad = (held, pulse) => {
    /* No layout (a stale bundle) reads no controller, but the on-screen
     * control still names its buttons directly. */
    if (!GP_LAYOUT && !virtualHeld.size && !gamepadHeld.size) return false;
    const { now, pressed, released } = pollGamepadEdges();
    flushCaptures();
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

  /* Controller rumble: the two motor levels the engine's frame left
   * (`play_rumble_levels`, the native window's gilrs force-feedback source
   * too) onto every connected pad with a `vibrationActuator`. A level that
   * holds is re-sent before the last effect runs out; a drop to zero stops
   * the motors. A browser without the haptics API rumbles nothing. */
  const RUMBLE_EFFECT_MS = 200;
  let rumbleLast = { strong: 0, weak: 0, at: 0 };
  window.legaiaSetRumble = (strong, weak) => {
    if (!navigator.getGamepads) return;
    const now = performance.now();
    const same = Math.abs(strong - rumbleLast.strong) < 1 / 64
      && Math.abs(weak - rumbleLast.weak) < 1 / 64;
    const idle = strong <= 0 && weak <= 0;
    if (same && (idle || now - rumbleLast.at < RUMBLE_EFFECT_MS / 2)) return;
    rumbleLast = { strong, weak, at: now };
    let pads = [];
    try { pads = Array.from(navigator.getGamepads() || []); } catch (e) { return; }
    for (const gp of pads) {
      const act = gp && gp.connected && gp.vibrationActuator;
      if (!act) continue;
      try {
        if (idle) {
          if (typeof act.reset === 'function') act.reset();
        } else {
          act.playEffect('dual-rumble', {
            duration: RUMBLE_EFFECT_MS, strongMagnitude: strong, weakMagnitude: weak,
          });
        }
      } catch (e) { /* a pad that refuses the effect keeps playing silently */ }
    }
  };

  /* What a prompt names for a pad button: the key, or the last pressed
   * controller's button in its maker's labels (`pad_prompt_label`,
   * `Mapping::prompt_label`); `''` against a bundle without it. */
  window.legaiaPadPromptFor = (name) => {
    if (!FOLD_SRC || typeof FOLD_SRC.pad_prompt_label !== 'function') return '';
    try { return FOLD_SRC.pad_prompt_label(name) || ''; } catch (e) { return ''; }
  };
  window.legaiaLastInputDevice = () => lastDevice;
  /* A key press hands the prompts back to the keyboard. */
  window.addEventListener('keydown', () => {
    if (lastDevice === 'keyboard') return;
    try { if (FOLD_SRC && typeof FOLD_SRC.pad_note_keyboard === 'function') FOLD_SRC.pad_note_keyboard(); } catch (e) {}
    noteDevice('keyboard');
  }, true);

  /* Hot-plug. Every connected controller drives the one player (their words
   * are ORed, as natively), so a pad picked up mid-game works at once; the
   * page says which came and went on its notice line, and a pad with no
   * `standard` mapping is told its buttons are a guess. `#play-pad-status`,
   * where a page has one, lists what is connected. A browser only reports a
   * controller after its first button press. */
  const shortId = (id) => String(id || 'controller').replace(/\s*\(.*$/, '').slice(0, 48);
  function padNotice(text) {
    if (typeof window.__playShowNotice === 'function') window.__playShowNotice(text);
    else console.info('[legaia] ' + text);
  }
  function padStatus() {
    const el = document.getElementById('play-pad-status');
    if (!el) return;
    let pads = [];
    try { pads = Array.from((navigator.getGamepads && navigator.getGamepads()) || []); } catch (e) {}
    const live = pads.filter((gp) => gp && gp.connected);
    el.textContent = live.length
      ? live.map((gp) => shortId(gp.id) + (gp.mapping === 'standard' ? '' : ' (buttons guessed)')).join(', ')
      : 'No controller (press a button on one)';
  }
  window.legaiaPadStatus = padStatus;
  window.addEventListener('gamepadconnected', (e) => {
    const gp = e.gamepad;
    padNotice('Controller connected: ' + shortId(gp.id)
      + (gp.mapping === 'standard' ? '' : ' - no standard layout, buttons guessed; set them in Options > Key Config'));
    padStatus();
  });
  window.addEventListener('gamepaddisconnected', (e) => {
    const gp = e.gamepad;
    padState.delete(padKey(gp));
    padNotice('Controller disconnected: ' + shortId(gp.id));
    padStatus();
  });

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
    swallowHeldPads();
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
