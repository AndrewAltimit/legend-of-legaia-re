/* Shared sound on/off gate for the site pages that produce audio
 * (minigames, media browser). One global flag persisted in localStorage,
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
 * Presentation-only: pages keep owning their AudioContexts; this is just
 * the shared switch in front of them. */
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
      'background:transparent;color:var(--text-muted, #9a96b8);';
    btn.addEventListener('click', function () { set(!enabled); });
    paint(btn);
    buttons.push(btn);
    return btn;
  }

  window.LegaiaSound = {
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
