/* rom-patcher-page-controls.js - the ROM patcher page's shared DOM helpers.
 *
 * The id lookup every patcher module uses, the "?" info-tooltip behaviour and
 * the segmented (radio-group) read / write pair.
 *
 * Module script. The page loads it first of the rom-patcher-*.js files; it
 * publishes on window.LegaiaRomPatcher, which the others read. */

function $(id) {
  return document.getElementById(id);
}

// --- Info tooltips ----------------------------------------------------------
// Every `.info-tip` is a "?" bead whose `.tip-pop` popover opens on hover /
// keyboard focus (pure CSS) and toggles on click or Enter/Space (for touch),
// with Escape and outside-clicks closing any pinned tip. Delegated on the
// document so the JS-built editor rows get the behavior for free. The bead is
// a span (not a <button>) so a tip may carry links, and being interactive
// content it never forwards a click to the checkbox of a wrapping <label>.
function setupInfoTips() {
  const openTips = () => document.querySelectorAll('.info-tip.is-open');
  const close = (tip) => {
    tip.classList.remove('is-open');
    tip.setAttribute('aria-expanded', 'false');
  };
  // Popovers center under the bead by default; near a viewport edge they pin
  // to the bead's side instead so they stay readable.
  const position = (tip) => {
    tip.classList.remove('tip-align-right', 'tip-align-left');
    const r = tip.getBoundingClientRect();
    const half = Math.min(384, window.innerWidth * 0.78) / 2;
    const mid = r.left + r.width / 2;
    if (mid + half > window.innerWidth - 12) tip.classList.add('tip-align-right');
    else if (mid - half < 12) tip.classList.add('tip-align-left');
  };
  const toggle = (tip) => {
    const open = !tip.classList.contains('is-open');
    openTips().forEach((t) => { if (t !== tip) close(t); });
    tip.classList.toggle('is-open', open);
    tip.setAttribute('aria-expanded', String(open));
    if (open) position(tip);
  };
  document.addEventListener('click', (e) => {
    const tip = e.target.closest('.info-tip');
    if (!tip) {
      openTips().forEach(close);
      return;
    }
    if (e.target.closest('a')) return; // links inside an open tip stay links
    // Inside a <summary> or <label>, a plain click would also toggle the
    // group / checkbox - the tip click is only ever about the tip.
    e.preventDefault();
    e.stopPropagation();
    toggle(tip);
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      openTips().forEach(close);
      return;
    }
    const tip = e.target.closest && e.target.closest('.info-tip');
    if (tip && (e.key === 'Enter' || e.key === ' ')) {
      e.preventDefault();
      toggle(tip);
    }
  });
  // Hover / focus opens via CSS; JS only fixes the edge alignment.
  document.addEventListener('mouseover', (e) => {
    const tip = e.target.closest && e.target.closest('.info-tip');
    if (tip && !tip.classList.contains('is-open')) position(tip);
  });
  document.addEventListener('focusin', (e) => {
    const tip = e.target.closest && e.target.closest('.info-tip');
    if (tip) position(tip);
  });
}

// --- Segmented (radio-group) helpers ---------------------------------------

// The checked value of a radio group, or `dflt` if none is set.
function segVal(name, dflt) {
  const el = document.querySelector(`input[name="${name}"]:checked`);
  return el ? el.value : dflt;
}

// Programmatically select a radio-group value (does not fire `change`).
function setSeg(name, value) {
  const el = document.querySelector(`input[name="${name}"][value="${value}"]`);
  if (el) el.checked = true;
}

window.LegaiaRomPatcher = Object.assign(window.LegaiaRomPatcher || {}, {
  $, setupInfoTips, segVal, setSeg,
});
