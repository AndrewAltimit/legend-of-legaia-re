/* Small helpers every page shares. Classic script (no type="module"),
 * loaded from the page <head> by the template in site/_gen.py, so inline page
 * scripts, classic scripts and modules alike reach it as a global. */
(function () {
  'use strict';
  var ENTITIES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };

  /* Escape text for HTML element content or a quoted attribute value.
   * `null` / `undefined` render as the empty string. */
  function escapeHtml(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, function (c) { return ENTITIES[c]; });
  }

  window.escapeHtml = escapeHtml;
})();
