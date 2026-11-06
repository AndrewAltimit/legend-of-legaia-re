/* touch-pad.js - an on-screen PSX pad for touch screens.
 *
 * A phone or tablet has no keyboard and usually no controller, so the play
 * page could show the game but not play it. This draws a D-pad, the four face
 * buttons, the shoulders and Start / Select over the picture, and holds each
 * pressed button through `window.legaiaVirtualPad` (pad-bindings.js). That
 * source joins the controller poll every page already folds into its pad
 * word, so a touch press reaches the field tick, the menus and the title card
 * exactly as a controller button does. No binding table here: the control
 * names pad buttons, and the engine's own button bits answer them.
 *
 * The D-pad is one surface, read by angle from its centre, so a thumb can
 * roll from Up to Up-Right to Right without lifting. Each other button holds
 * while any finger is down on it.
 *
 * Global API (`window.LegaiaTouchPad`):
 *   install(wrap, opts)  build the pad inside `wrap` (positioned); returns
 *                        { setShown(bool), shown() }. `opts.onPress()` runs
 *                        on every first touch (the page takes focus and
 *                        unlocks audio there, as a canvas click does).
 *   wantedByDefault()    true on a device whose only pointer is coarse.
 *
 * Classic script. Requires pad-bindings.js first. */
(function () {
  'use strict';

  const FACE = [
    ['Triangle', '△'], ['Circle', '○'], ['Cross', '✕'], ['Square', '□'],
  ];
  const SHOULDERS = [['L2', 'L2'], ['L1', 'L1'], ['R1', 'R1'], ['R2', 'R2']];
  const MIDDLE = [['Select', 'SELECT'], ['Start', 'START']];
  /* Inside this fraction of the D-pad's radius nothing is held, so a thumb
   * resting on the centre does not walk. */
  const DEAD = 0.22;
  /* A direction's axis counts while its share of the offset is above this:
   * 0.38 gives each diagonal about a 45-degree wedge. */
  const AXIS = 0.38;

  function pad() { return window.legaiaVirtualPad; }

  function wantedByDefault() {
    try {
      return matchMedia('(pointer: coarse)').matches && !matchMedia('(any-pointer: fine)').matches;
    } catch (e) { return false; }
  }

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text) e.textContent = text;
    return e;
  }

  /* A held button: pressed while any pointer is down on it. */
  function holdButton(node, name, onPress) {
    const down = new Set();
    const set = (on) => {
      node.classList.toggle('is-held', on);
      if (!pad()) return;
      if (on) pad().press(name); else pad().release(name);
    };
    node.addEventListener('pointerdown', (e) => {
      /* No focus move, no synthetic mouse events, no scroll. */
      e.preventDefault();
      try { node.setPointerCapture(e.pointerId); } catch (_) {}
      if (!down.size) set(true);
      down.add(e.pointerId);
      if (onPress) onPress();
    });
    const up = (e) => {
      if (!down.delete(e.pointerId)) return;
      if (!down.size) set(false);
    };
    node.addEventListener('pointerup', up);
    node.addEventListener('pointercancel', up);
    node.addEventListener('lostpointercapture', up);
    return () => { down.clear(); set(false); };
  }

  /* The D-pad: one surface, the held directions from the pointer's angle. */
  function holdDpad(node, onPress) {
    let pointer = null;
    let held = new Set();
    const apply = (next) => {
      for (const d of held) if (!next.has(d) && pad()) pad().release(d);
      for (const d of next) if (!held.has(d) && pad()) pad().press(d);
      held = next;
      for (const d of ['Up', 'Down', 'Left', 'Right']) {
        node.classList.toggle('is-' + d.toLowerCase(), held.has(d));
      }
    };
    const read = (e) => {
      const r = node.getBoundingClientRect();
      const rad = Math.min(r.width, r.height) / 2;
      const x = (e.clientX - (r.left + r.width / 2)) / rad;
      const y = (e.clientY - (r.top + r.height / 2)) / rad;
      const len = Math.hypot(x, y);
      const next = new Set();
      if (len >= DEAD) {
        if (x / len > AXIS) next.add('Right');
        if (x / len < -AXIS) next.add('Left');
        if (y / len > AXIS) next.add('Down');
        if (y / len < -AXIS) next.add('Up');
      }
      apply(next);
    };
    node.addEventListener('pointerdown', (e) => {
      e.preventDefault();
      if (pointer !== null) return;
      pointer = e.pointerId;
      try { node.setPointerCapture(e.pointerId); } catch (_) {}
      read(e);
      if (onPress) onPress();
    });
    node.addEventListener('pointermove', (e) => { if (e.pointerId === pointer) read(e); });
    const up = (e) => {
      if (e.pointerId !== pointer) return;
      pointer = null;
      apply(new Set());
    };
    node.addEventListener('pointerup', up);
    node.addEventListener('pointercancel', up);
    node.addEventListener('lostpointercapture', up);
    return () => { pointer = null; apply(new Set()); };
  }

  function install(wrap, opts) {
    if (!wrap) return null;
    const onPress = opts && opts.onPress;
    const root = el('div', 'touch-pad is-hidden');
    root.setAttribute('aria-hidden', 'true');
    const releases = [];

    const shoulders = el('div', 'tp-shoulders');
    for (const [name, label] of SHOULDERS) {
      const b = el('div', 'tp-btn tp-shoulder tp-' + name.toLowerCase(), label);
      releases.push(holdButton(b, name, onPress));
      shoulders.appendChild(b);
    }
    root.appendChild(shoulders);

    const dpad = el('div', 'tp-dpad');
    for (const d of ['up', 'right', 'down', 'left']) dpad.appendChild(el('span', 'tp-arm tp-arm-' + d));
    releases.push(holdDpad(dpad, onPress));
    root.appendChild(dpad);

    const middle = el('div', 'tp-middle');
    for (const [name, label] of MIDDLE) {
      const b = el('div', 'tp-btn tp-small', label);
      releases.push(holdButton(b, name, onPress));
      middle.appendChild(b);
    }
    root.appendChild(middle);

    const face = el('div', 'tp-face');
    for (const [name, label] of FACE) {
      const b = el('div', 'tp-btn tp-round tp-' + name.toLowerCase(), label);
      b.title = name;
      releases.push(holdButton(b, name, onPress));
      face.appendChild(b);
    }
    root.appendChild(face);

    /* A long press must not open the browser's context menu over the game. */
    root.addEventListener('contextmenu', (e) => e.preventDefault());
    wrap.appendChild(root);

    let shown = false;
    const setShown = (on) => {
      shown = !!on;
      root.classList.toggle('is-hidden', !shown);
      /* Hiding the pad under a held thumb must not leave the button held. */
      if (!shown) for (const r of releases) r();
    };
    return { setShown, shown: () => shown };
  }

  window.LegaiaTouchPad = { install, wantedByDefault };
})();
