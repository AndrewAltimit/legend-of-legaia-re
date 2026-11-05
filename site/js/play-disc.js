/* play-disc.js - the play page's disc image, read as it is needed.
 *
 * Classic script (no `import` / `export` / `import.meta` - see
 * docs/tooling/site-shell.md). Defines `window.LegaiaPlayDisc`.
 *
 * Only the sectors up to the end of PROT.DAT are read before the engine comes
 * up; they cross into the runtime (`load_disc_spans`) and are let go. The XA and
 * movie files behind them stay in the `Blob`, which is disk-backed, and are read
 * by span when a consumer names one: `span` answers null until the read lands,
 * and the consumer asks again next frame. What has landed stays resident under
 * `RESIDENT_CAP`, oldest use out first.
 *
 * Three ways to hold bytes across a wait, and eviction can break none of them:
 * a window `prefetch` named is kept until its consumer calls `release`; `read`
 * answers with the bytes themselves; a read in flight is not in `blocks` to be
 * evicted at all. A read that lands with nobody left waiting (a movie past its
 * install timeout) is dropped.
 *
 * Anything that is not a Mode 2/2352 image with a PROT.DAT in its root (a bare
 * PROT.DAT, an unknown image, a source with no Blob) is read whole and handed
 * to `load_disc_image`, which decides what it is. */
(function () {
  'use strict';

  /* A raw sector, and where its user data sits. Paired with `RAW` and
   * `DATA_OFFSET` in crates/web-play/src/runtime/disc_load.rs. */
  const RAW = 2352, DATA_OFFSET = 24, DATA = 2048;
  const EMPTY = new Uint8Array(0);
  /* What may stay resident once its consumers are done, in bytes. Room for the
   * largest XA file (18.3 MiB) beside the largest set of clips one scene reads
   * ahead (11.3 MiB, jagaroom), the two a repeat can hit. A movie is dropped at
   * its install and a voice bank never lands, so neither needs room here; a window
   * a consumer holds is outside the cap. */
  const RESIDENT_CAP = 32 * 1048576;
  /* A read that fails is tried this many times, this far apart in ms. */
  const READ_TRIES = 3, READ_RETRY_MS = 250;

  function blobOf(source) {
    if (source instanceof Blob) return source;
    return source && source.blob instanceof Blob ? source.blob : null;
  }

  function readBlob(blob, start, end) {
    return blob.slice(start, end).arrayBuffer().then((ab) => new Uint8Array(ab));
  }

  const u32 = (b, o) => (b[o] | b[o + 1] << 8 | b[o + 2] << 16 | b[o + 3] << 24) >>> 0;

  /* The root directory's PROT.DAT and DMY.DAT, as `{ protEnd, dmyLba }` in
   * sectors. The runtime's own walk stays the authority on the image
   * (`disc_image_missing`, `load_disc_spans`); this only says how much to read
   * first, and the wasm module is still compiling when it runs. */
  async function layoutOf(blob) {
    const size = blob.size;
    if (size < 17 * RAW || size % RAW) return null;
    const pvd = await readBlob(blob, 16 * RAW, 17 * RAW);
    const magic = String.fromCharCode(...pvd.subarray(DATA_OFFSET + 1, DATA_OFFSET + 6));
    if (pvd[DATA_OFFSET] !== 1 || magic !== 'CD001') return null;
    const rootLba = u32(pvd, DATA_OFFSET + 156 + 2);
    const rootSectors = Math.ceil(u32(pvd, DATA_OFFSET + 156 + 10) / DATA);
    if (!rootSectors || rootSectors > 64 || (rootLba + rootSectors) * RAW > size) return null;
    const root = await readBlob(blob, rootLba * RAW, (rootLba + rootSectors) * RAW);
    const found = {};
    for (let s = 0; s < rootSectors; s++) {
      const base = s * RAW + DATA_OFFSET;
      for (let o = 0; o < DATA;) {
        const len = root[base + o];
        if (!len) break;
        const nameLen = root[base + o + 32];
        const name = String.fromCharCode(...root.subarray(base + o + 33, base + o + 33 + nameLen));
        found[name.split(';')[0]] = {
          lba: u32(root, base + o + 2),
          sectors: Math.ceil(u32(root, base + o + 10) / DATA),
        };
        o += len;
      }
    }
    const prot = found['PROT.DAT'];
    if (!prot || (prot.lba + prot.sectors) * RAW > size) return null;
    return { protEnd: prot.lba + prot.sectors, dmyLba: found['DMY.DAT'] ? found['DMY.DAT'].lba : 0 };
  }

  class PlayDisc {
    constructor(blob, layout) {
      this.blob = blob;
      this.layout = layout;
      this.size = blob ? blob.size : 0;
      /* What is resident: `{ start, end, bytes }` in file bytes, oldest use first. */
      this.blocks = [];
      /* The sectors up to PROT.DAT's end, until the runtime has its copy. */
      this.head = null;
      /* The whole image, when it is not one this file can read in parts. */
      this.whole = null;
      /* The reads in flight: `{ start, end, land, promise }`. */
      this.reads = new Set();
      /* The windows consumers wait on: `start:end` -> `{ start, end, failed }`. */
      this.holds = new Map();
      this.closed = false;
    }

    /* The image's first read. `prog` is the page's LoadProgress. `beforeWhole`
     * runs ahead of reading a file whole: it may throw, so a 2 GB archive is
     * refused without reading it. */
    static async open(source, prog, text, beforeWhole) {
      const blob = blobOf(source);
      let layout = null;
      if (blob) {
        try { layout = await layoutOf(blob); } catch (e) { layout = null; }
      }
      if (!layout && beforeWhole) await beforeWhole();
      const disc = new PlayDisc(blob, layout);
      if (layout) {
        disc.head = await prog.read(blob.slice(0, layout.protEnd * RAW), text);
      } else {
        disc.whole = await prog.read(source, text);
        disc.size = disc.whole.length;
        disc.blocks.push({ start: 0, end: disc.size, bytes: disc.whole });
      }
      return disc;
    }

    /* Build `rt`'s session over the image: the disc load, or a rebuild after
     * a trap (the head is read again; the head is let go after the load). */
    async loadInto(rt) {
      if (this.whole) return rt.load_disc_image(this.whole, '');
      const head = this.head || await readBlob(this.blob, 0, this.layout.protEnd * RAW);
      this.head = null;
      const lbas = [0], blocks = [head];
      /* The runtime's walk names any sector the head left out: a directory or
       * a root file past PROT.DAT, on an image laid out unlike the retail one. */
      for (let round = 0; ; round++) {
        const miss = rt.disc_image_missing(this.size, Uint32Array.from(lbas), blocks);
        if (!miss.length) break;
        if (round > 256) throw new Error('disc walk does not settle');
        lbas.push(miss[0]);
        blocks.push(await readBlob(this.blob, miss[0] * RAW, (miss[0] + miss[1]) * RAW));
      }
      return rt.load_disc_spans(this.size, Uint32Array.from(lbas), blocks, '');
    }

    /* The window `lba` + `sectors` names, clamped to the image as a subarray
     * of the image would be. */
    _extent(lba, sectors) {
      const start = lba * RAW;
      return [start, Math.min(this.size, start + sectors * RAW)];
    }

    /* Those raw sectors if they are resident, else null. An empty array for a
     * window wholly past the image's end, where there is nothing to wait for.
     * The one block covering the whole window answers: neighbours are not
     * stitched, so a window across two blocks reads again. */
    span(lba, sectors) {
      const [start, end] = this._extent(lba, sectors);
      if (start >= end) return EMPTY;
      for (let i = this.blocks.length - 1; i >= 0; i--) {
        const b = this.blocks[i];
        if (b.start <= start && end <= b.end) {
          if (i !== this.blocks.length - 1) {
            this.blocks.splice(i, 1);
            this.blocks.push(b);
          }
          return b.bytes.subarray(start - b.start, end - b.start);
        }
      }
      return null;
    }

    /* Make those sectors resident and keep them so until `release`. The one read
     * of a window settles every asker, and a read already covering it serves.
     * A window that will not read (the file moved, say) stays out until its
     * `release`; `failedSpan` tells the consumer so it can stop waiting. */
    prefetch(lba, sectors) {
      const [start, end] = this._extent(lba, sectors);
      if (start >= end || !this.blob) return Promise.resolve();
      const key = start + ':' + end;
      const hold = this.holds.get(key);
      if (!hold) this.holds.set(key, { start, end, failed: false });
      else if (hold.failed) return Promise.resolve();
      if (this.span(lba, sectors)) return Promise.resolve();
      const rec = this._covering(start, end) || this._begin(start, end);
      rec.land = true;
      return rec.promise.then(() => {});
    }

    /* The consumer of a `prefetch` window is done: what it held is evictable, or
     * gone with `drop`, and a failure is forgotten so the next ask reads again. */
    release(lba, sectors, drop) {
      const [start, end] = this._extent(lba, sectors);
      this.holds.delete(start + ':' + end);
      if (drop) {
        this.blocks = this.blocks.filter((b) => b.bytes === this.whole || this._held(b)
          || !(b.start <= start && end <= b.end));
      }
      this._evict();
    }

    failedSpan(lba, sectors) {
      const [start, end] = this._extent(lba, sectors);
      const hold = this.holds.get(start + ':' + end);
      return !!(hold && hold.failed);
    }

    /* Those raw sectors as a promise, for a consumer that uses them once and
     * right away: null if they will not read. They are not kept resident for it. */
    read(lba, sectors) {
      const [start, end] = this._extent(lba, sectors);
      if (start >= end) return Promise.resolve(EMPTY);
      const resident = this.span(lba, sectors);
      if (resident) return Promise.resolve(resident);
      if (!this.blob) return Promise.resolve(null);
      const rec = this._covering(start, end) || this._begin(start, end);
      return rec.promise.then((bytes) => bytes && bytes.subarray(start - rec.start, end - rec.start));
    }

    _covering(start, end) {
      for (const r of this.reads) if (r.start <= start && end <= r.end) return r;
      return null;
    }

    _begin(start, end) {
      const rec = { start, end, land: false, promise: null };
      rec.promise = this._readTried(start, end).then((bytes) => {
        this.reads.delete(rec);
        if (rec.land && !this.closed) this._land(rec, bytes);
        return bytes;
      }, (e) => {
        this.reads.delete(rec);
        console.warn('disc read', start, end, e);
        for (const h of this.holds.values()) {
          if (rec.start <= h.start && h.end <= rec.end) h.failed = true;
        }
        return null;
      });
      this.reads.add(rec);
      return rec;
    }

    async _readTried(start, end) {
      for (let tried = 1; ; tried++) {
        try {
          const bytes = await readBlob(this.blob, start, end);
          if (bytes.length !== end - start) throw new Error(`short read: ${bytes.length} of ${end - start}`);
          return bytes;
        } catch (e) {
          if (tried >= READ_TRIES || this.closed) throw e;
          await new Promise((res) => setTimeout(res, READ_RETRY_MS * tried));
        }
      }
    }

    /* A read arrives. Dropped when a consumer no longer waits on any of it or a
     * block already covers it (a narrower read that a wider one beat); blocks it
     * covers go, since it answers for them. */
    _land(rec, bytes) {
      const { start, end } = rec;
      const block = { start, end, bytes };
      if (!this._held(block)) return;
      if (this.blocks.some((b) => b.start <= start && end <= b.end)) return;
      this.blocks = this.blocks.filter((b) => b.bytes === this.whole || b.start < start || b.end > end);
      this.blocks.push(block);
      this._evict();
    }

    _held(block) {
      for (const h of this.holds.values()) {
        if (block.start <= h.start && h.end <= block.end) return true;
      }
      return false;
    }

    _evict() {
      let total = 0;
      for (const b of this.blocks) if (b.bytes !== this.whole) total += b.end - b.start;
      if (total <= RESIDENT_CAP) return;
      this.blocks = this.blocks.filter((b) => {
        if (total <= RESIDENT_CAP || b.bytes === this.whole || this._held(b)) return true;
        total -= b.end - b.start;
        return false;
      });
    }

    /* A new disc replaced this one. */
    close() {
      this.closed = true;
      this.blocks = [];
      this.holds.clear();
      this.head = null;
    }
  }

  window.LegaiaPlayDisc = { open: PlayDisc.open, RAW: RAW, RESIDENT_CAP: RESIDENT_CAP };
}());
