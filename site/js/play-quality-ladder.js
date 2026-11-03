/* play-quality-ladder.js - when the page steps its own picture down.
 *
 * The decision alone, with no GL, DOM or engine in it: PlayView (play-app.js)
 * feeds it one sample per animation frame and applies the rung it names. It
 * only ever steps down, one rung at a time, within one page session; nothing
 * it does reaches the persisted options, because it holds no way to write them.
 *
 * A step needs two whole windows in a row whose MEDIAN frame gap misses the
 * cadence, so a scene entry's stall, a battle edge or one slow second cannot
 * cause one. A gap over `stallMs` (a hidden tab, a long load) restarts the
 * count: it is evidence of neither a slow GPU nor a fast one. A window whose
 * frame function already takes half the gap is left alone: the main thread is
 * what is short, and no rung here relieves it.
 *
 * Classic script, also loaded by scripts/ci/quality-ladder-web.mjs under node. */
(function (root) {
  'use strict';

  const DEFAULTS = {
    /* A median gap past this is under 25 fps, which leaves a 30 Hz display alone. */
    missMs: 40,
    windowMs: 4000,
    /* Two windows: at least eight seconds of a missed cadence. */
    windowsToStep: 2,
    /* What a step's own reallocation costs is not counted against the next one. */
    settleMs: 2000,
    stallMs: 1000,
    /* The main thread's share of the gap at which the GPU is not the limit. */
    mainThreadShare: 0.5,
    capacity: 1024,
  };

  const median = (a, n) => {
    const s = a.slice(0, n).sort();
    return s[n >> 1];
  };

  class QualityLadder {
    /* `rungs`: `[{ id, applicable() -> bool, apply() }]`, in the order to spend
     * them. `exempt()`: true while the ladder must not act. */
    constructor(rungs, exempt, config) {
      this.cfg = Object.assign({}, DEFAULTS, config);
      this.rungs = rungs;
      this.exempt = exempt;
      this.armed = true;
      this.stepped = [];
      this.chosen = new Set();
      this.gaps = new Float32Array(this.cfg.capacity);
      this.costs = new Float32Array(this.cfg.capacity);
      this.last = null;
      this._restart(0);
    }

    _restart(settleMs) {
      this.n = 0;
      this.held = 0;
      this.misses = 0;
      this.settle = settleMs;
    }

    /* The player decided about this rung: it is theirs for the session. */
    choose(id) {
      this.chosen.add(id);
    }

    /* Stop for the session. What was stepped is the caller's to put back. */
    disarm() {
      this.armed = false;
    }

    _next() {
      for (const r of this.rungs) {
        if (this.stepped.includes(r.id) || this.chosen.has(r.id)) continue;
        if (r.applicable()) return r;
      }
      return null;
    }

    /* One animation frame: `now` its start, `costMs` its frame function, and
     * `counts` false for a frame that says nothing of the GPU (paused, a movie).
     * Returns the rung it stepped, or null. */
    sample(now, costMs, counts) {
      const gap = this.last === null ? null : now - this.last;
      this.last = now;
      if (!this.armed || gap === null) return null;
      if (!counts || gap < 0 || gap > this.cfg.stallMs || this.exempt()) {
        this._restart(0);
        return null;
      }
      if (this.settle > 0) {
        this.settle -= gap;
        return null;
      }
      if (this.n < this.cfg.capacity) {
        this.gaps[this.n] = gap;
        this.costs[this.n] = costMs;
        this.n++;
      }
      this.held += gap;
      if (this.held < this.cfg.windowMs) return null;
      const g = median(this.gaps, this.n);
      const missed = g > this.cfg.missMs && median(this.costs, this.n) < g * this.cfg.mainThreadShare;
      this.misses = missed ? this.misses + 1 : 0;
      this.n = 0;
      this.held = 0;
      if (this.misses < this.cfg.windowsToStep) return null;
      const rung = this._next();
      if (!rung) {
        this.armed = false;
        return null;
      }
      this.stepped.push(rung.id);
      rung.apply();
      this._restart(this.cfg.settleMs);
      return rung.id;
    }
  }

  root.LegaiaQualityLadder = QualityLadder;
})(typeof window !== 'undefined' ? window : globalThis);
