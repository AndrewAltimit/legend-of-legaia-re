/* Shared sound on/off gate for the site pages that produce audio
 * (minigames, media browser, arts, play). One global flag persisted in localStorage,
 * a reusable speaker-icon toggle button, and a tiny gate API:
 *
 *   LegaiaSound.isSoundOn()      -> bool; call at every point that would
 *                                   actually start audio and skip when false
 *   LegaiaSound.setSoundOn(on)   -> flip programmatically
 *   LegaiaSound.toggle()
 *   LegaiaSound.onChange(cb)     -> cb(enabled) after every flip; use it to
 *                                   stop already-playing sources on mute
 *   LegaiaSound.attach(el)       -> append a toggle button into `el`
 *                                   (all attached buttons stay in sync)
 *
 * and the page's ONE AudioContext, which every sound source on the page
 * (WASM SPU outputs, rendered BGM loops, cue buffers, XA clips) sits on:
 *
 *   LegaiaSound.context()        -> the shared AudioContext, or null until the
 *                                   page has had a user gesture. Built lazily
 *                                   on the first call after one, resumed on
 *                                   every call; never created or resumed
 *                                   before a gesture, so the browser's autoplay
 *                                   policy has nothing to refuse
 *   LegaiaSound.unlocked()       -> has the page had its gesture?
 *   LegaiaSound.onUnlock(cb)     -> cb() once, on the first gesture (at once if
 *                                   it already happened): open a WASM output
 *                                   or start a track that a pre-gesture call
 *                                   had to skip
 *   LegaiaSound.addResumer(fn)   -> fn() on every gesture, for a context this
 *                                   module does not own (an older WASM bundle
 *                                   that builds its own)
 *   LegaiaSound.buffer(ch, n, r) -> an AudioBuffer, with or without the
 *                                   context (an AudioBuffer belongs to no
 *                                   context, so pre-rendering needs no gesture)
 *
 * Every gesture (pointerdown, mousedown, touchend, keydown, click; capture
 * phase on window, so it runs ahead of the page's own handler) resumes the
 * context. A browser can suspend a running context later (Safari on an
 * interruption, a tab coming back), so the resume is not once-only. A gamepad
 * press is not a user activation in any browser, so a pad alone cannot
 * unlock audio; the page's first click or key does. */
(function () {
  'use strict';

  var KEY = 'legaia-sound-enabled';
  var enabled = true;
  try { enabled = window.localStorage.getItem(KEY) !== '0'; } catch (e) { /* private mode */ }

  var listeners = [];
  var buttons = [];

  /* Inline SVG speaker (self-contained; no font/emoji dependency). */
  function iconSvg(on) {
    var base = '<path d="M3 6h3l4-3.5v11L6 10H3z" fill="currentColor"/>';
    var waves = on
      ? '<path d="M11.5 5.2a3.4 3.4 0 0 1 0 5.6M13 3.4a5.8 5.8 0 0 1 0 9.2"' +
        ' fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>'
      : '<path d="M11 6l4 4M15 6l-4 4"' +
        ' fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>';
    return '<svg viewBox="0 0 17 16" width="14" height="14" aria-hidden="true"' +
      ' style="vertical-align:-2px;">' + base + waves + '</svg>';
  }

  function paint(btn) {
    btn.innerHTML = iconSvg(enabled) + ' ' + (enabled ? 'Sound on' : 'Sound off');
    btn.setAttribute('aria-pressed', enabled ? 'true' : 'false');
    btn.title = enabled ? 'Sound is on - click to mute' : 'Sound is off - click to unmute';
    btn.style.opacity = enabled ? '1' : '0.6';
  }

  function set(on) {
    on = !!on;
    if (on === enabled) return;
    enabled = on;
    try { window.localStorage.setItem(KEY, on ? '1' : '0'); } catch (e) { /* ignore */ }
    for (var i = 0; i < buttons.length; i++) paint(buttons[i]);
    for (var j = 0; j < listeners.length; j++) {
      try { listeners[j](enabled); } catch (e) { console.warn('[sound] onChange cb', e); }
    }
  }

  function makeButton() {
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'sound-toggle';
    /* Self-styled so pages don't each need CSS; inherits the site palette
     * through the CSS variables every page defines. */
    btn.style.cssText =
      'display:inline-flex;align-items:center;gap:0.35rem;' +
      'padding:0.3rem 0.7rem;margin-left:auto;cursor:pointer;' +
      'font:inherit;font-size:0.82rem;border-radius:4px;' +
      'border:1px solid var(--border, #4a4668);' +
      'background:transparent;color:var(--text-muted, #9a96b8);' +
      /* A finger's target on a touch screen. */
      (window.matchMedia && matchMedia('(pointer: coarse)').matches ? 'min-height:44px;' : '');
    btn.addEventListener('click', function () { set(!enabled); });
    paint(btn);
    buttons.push(btn);
    return btn;
  }

  /* ---- the page's one AudioContext ---- */

  var ctx = null;
  var gestured = false;
  var unlockCbs = [];
  var resumers = [];

  function unlocked() {
    if (gestured) return true;
    var ua = navigator.userActivation;
    if (ua && ua.hasBeenActive) return true;
    /* An embedded same-origin page (the arts / magic battle frames, the
     * workbench preview) takes its gesture in the parent, which delegates
     * autoplay to it (`allow="autoplay"`). */
    try {
      if (window.parent !== window && window.parent.navigator.userActivation
          && window.parent.navigator.userActivation.hasBeenActive) return true;
    } catch (e) { /* cross-origin parent */ }
    return false;
  }

  function resumeCtx() {
    if (ctx && ctx.state !== 'running' && ctx.state !== 'closed' && ctx.resume) {
      try { var p = ctx.resume(); if (p && p.catch) p.catch(function () {}); } catch (e) { /* refused */ }
    }
  }

  function context() {
    if (!unlocked()) return null;
    if (!ctx) {
      var Ctor = window.AudioContext || window.webkitAudioContext;
      if (!Ctor) return null;
      try { ctx = new Ctor(); } catch (e) { return null; }
    }
    resumeCtx();
    return ctx;
  }

  function onGesture(e) {
    /* A key the browser does not count as activation (Escape, a lone
     * modifier) must not open the context: it would come up suspended. */
    var ua = navigator.userActivation;
    if (ua && !ua.isActive && !ua.hasBeenActive) return;
    if (e && e.isTrusted === false) return;
    var first = !gestured;
    gestured = true;
    resumeCtx();
    for (var i = 0; i < resumers.length; i++) {
      try { resumers[i](); } catch (err) { /* a stale output */ }
    }
    if (first) {
      var cbs = unlockCbs; unlockCbs = [];
      for (var j = 0; j < cbs.length; j++) {
        try { cbs[j](); } catch (err) { console.warn('[sound] onUnlock cb', err); }
      }
    }
  }
  ['pointerdown', 'mousedown', 'touchend', 'keydown', 'click'].forEach(function (t) {
    window.addEventListener(t, onGesture, { capture: true, passive: true });
  });

  function makeBuffer(channels, frames, rate) {
    if (ctx) return ctx.createBuffer(channels, frames, rate);
    if (typeof window.AudioBuffer === 'function') {
      try {
        return new window.AudioBuffer({ numberOfChannels: channels, length: frames, sampleRate: rate });
      } catch (e) { /* an engine without the constructor */ }
    }
    var c = context();
    return c ? c.createBuffer(channels, frames, rate) : null;
  }

  window.LegaiaSound = {
    context: context,
    unlocked: unlocked,
    onUnlock: function (cb) {
      if (typeof cb !== 'function') return;
      if (gestured || unlocked()) { gestured = true; cb(); } else unlockCbs.push(cb);
    },
    addResumer: function (fn) { if (typeof fn === 'function') resumers.push(fn); },
    buffer: makeBuffer,
    isSoundOn: function () { return enabled; },
    setSoundOn: set,
    toggle: function () { set(!enabled); },
    onChange: function (cb) { if (typeof cb === 'function') listeners.push(cb); },
    button: makeButton,
    attach: function (el) {
      if (typeof el === 'string') el = document.getElementById(el);
      if (!el) return null;
      var btn = makeButton();
      el.appendChild(btn);
      return btn;
    },
  };
})();
