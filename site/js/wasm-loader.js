/* The one loader for the site's WASM bundle (site/wasm/, built from
 * crates/web-viewer). Every page and script reaches the module through
 * `LegaiaWasm.load()`, which returns a promise of the initialised module.
 *
 * Why one loader rather than an `import()` per call site:
 *  - A dynamic import is keyed by its URL. Two sites on one page that spell
 *    the URL differently (one with `?v=`, one without) get two module
 *    instances and instantiate the whole bundle twice.
 *  - `?v=` (window.LEGAIA_WASM_V, injected by site/_gen.py) must ride on
 *    BOTH the glue import and the `_bg.wasm` fetch, or a deploy can pair new
 *    glue with a cached binary.
 *  - Paths resolve against this script's own URL, so the loader works from a
 *    page at any depth and from classic scripts, where `import.meta` is a
 *    syntax error.
 *
 * Classic script (no type="module"), loaded from the page <head> by the
 * template in site/_gen.py. */
(function () {
  'use strict';
  var base = document.currentScript ? document.currentScript.src : document.baseURI;
  var promise = null;

  function load() {
    if (!promise) {
      var v = window.LEGAIA_WASM_V || '0';
      var glue = new URL('../wasm/legaia_web_viewer.js?v=' + v, base).href;
      var bin = new URL('../wasm/legaia_web_viewer_bg.wasm?v=' + v, base).href;
      promise = import(glue).then(function (mod) {
        return mod.default(bin).then(function () { return mod; });
      });
      /* A failed load is not cached: the next call retries. */
      promise.catch(function () { promise = null; });
    }
    return promise;
  }

  window.LegaiaWasm = { load: load };
})();
