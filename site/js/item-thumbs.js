/* Equipment item thumbnails: one piece of equipment drawn on its own.
 *
 * Shared by the characters page (its equipment panel's card grid) and the
 * shops page (a render beside each weapon / armour row). The pixels come from
 * the WASM software rasteriser (`equipment_item_card_json` +
 * `equipment_item_card_pixels`): the character's battle model assembled
 * wearing exactly that one item, the item-alone cut framed on its own axes.
 * No WebGL context is involved, so a grid of forty cards costs no GPU
 * contexts; each build is about one loadout assembly, so builds go through a
 * queue that yields to the page between cards, and every result is cached for
 * the session.
 *
 *   const thumbs = new ItemThumbs(() => viewer, { size: 96 });
 *   const entry = thumbs.ensure(cslot, section, id);     // sync build, cached
 *   thumbs.paint(canvas, entry);
 *   thumbs.enqueue({ cslot, section, id, done: (entry) => ... });
 *   thumbs.observe(el, job);                             // build when visible
 *   thumbs.cancel();                                     // drop pending jobs
 *   thumbs.clear();                                      // new disc
 *
 * `entry` = { json, pixels (Uint8ClampedArray | null), size }. `json` is the
 * card metadata (`ok`, `name`, `class`, `isolation`, ...) or null.
 */
(function () {
  'use strict';

  class ItemThumbs {
    constructor(getViewer, opts) {
      const o = opts || {};
      this.getViewer = getViewer;
      this.size = o.size || 96;
      this.cache = new Map();
      this._queue = [];
      this._token = 0;
      this._pumping = false;
      this._io = null;
      this._watched = new Map();   /* element -> job */
    }

    static key(cslot, section, id) { return `${cslot}:${section}:${id}`; }

    /* True when the loaded WASM bundle can draw cards at all. */
    supported() {
      const v = this.getViewer();
      return !!(v && typeof v.equipment_item_card_json === 'function'
        && typeof v.equipment_item_card_pixels === 'function');
    }

    cached(cslot, section, id) {
      return this.cache.get(ItemThumbs.key(cslot, section, id));
    }

    /* Build (or return the cached) card. Synchronous: one assembly in WASM. */
    ensure(cslot, section, id) {
      const key = ItemThumbs.key(cslot, section, id);
      if (this.cache.has(key)) return this.cache.get(key);
      if (!this.supported()) return null;
      const viewer = this.getViewer();
      let json = null;
      try { json = JSON.parse(viewer.equipment_item_card_json(cslot, section, id)); } catch (e) { json = null; }
      let pixels = null;
      if (json && json.ok) {
        const px = viewer.equipment_item_card_pixels(this.size);
        if (px && px.length === this.size * this.size * 4) pixels = new Uint8ClampedArray(px);
      }
      const entry = { json, pixels, size: this.size };
      this.cache.set(key, entry);
      return entry;
    }

    /* Draw an entry into a canvas sized `size x size`. Clears `is-pending`
     * either way; returns whether there were pixels to draw. */
    paint(canvas, entry) {
      if (!canvas) return false;
      canvas.classList.remove('is-pending');
      if (entry && entry.pixels) {
        canvas.getContext('2d').putImageData(new ImageData(entry.pixels, entry.size, entry.size), 0, 0);
        return true;
      }
      canvas.title = 'this piece did not assemble on its own';
      return false;
    }

    /* Queue a build; `job.done(entry)` runs once it is ready (immediately
     * when already cached). Jobs run one per macrotask. */
    enqueue(job, front) {
      const hit = this.cached(job.cslot, job.section, job.id);
      if (hit) { if (job.done) job.done(hit); return; }
      if (front) this._queue.unshift(job); else this._queue.push(job);
      this._kick();
    }

    _kick() {
      if (this._pumping) return;
      this._pumping = true;
      const token = this._token;
      const pump = () => {
        if (token !== this._token) return;
        const next = this._queue.shift();
        if (!next) { this._pumping = false; return; }
        const entry = this.ensure(next.cslot, next.section, next.id);
        if (next.done) {
          try { next.done(entry); } catch (e) { console.warn('item thumb:', e); }
        }
        setTimeout(pump, 0);
      };
      setTimeout(pump, 0);
    }

    /* Drop every pending job (a re-render replaced the cards they were for). */
    cancel() {
      this._token++;
      this._queue = [];
      this._pumping = false;
      if (this._io) this._io.disconnect();
      this._watched.clear();
    }

    /* Forget every cached card (a different disc was loaded). */
    clear() {
      this.cancel();
      this.cache.clear();
    }

    /* Lazy build: queue `job` once `el` scrolls near the viewport. */
    observe(el, job) {
      const hit = this.cached(job.cslot, job.section, job.id);
      if (hit) { if (job.done) job.done(hit); return; }
      if (typeof IntersectionObserver === 'undefined') { this.enqueue(job); return; }
      if (!this._io) {
        this._io = new IntersectionObserver((entries) => {
          for (const e of entries) {
            if (!e.isIntersecting) continue;
            const j = this._watched.get(e.target);
            this._io.unobserve(e.target);
            this._watched.delete(e.target);
            if (j) this.enqueue(j);
          }
        }, { rootMargin: '200px 0px' });
      }
      this._watched.set(el, job);
      this._io.observe(el);
    }
  }

  window.ItemThumbs = ItemThumbs;
})();
