/* play-disc.js - the play page's disc image, read as it is needed.
 *
 * Classic script (no `import` / `export` / `import.meta` - see
 * docs/tooling/site-shell.md). Defines `window.LegaiaPlayDisc`.
 *
 * Only the sectors up to the end of PROT.DAT are read before the engine comes
 * up; they cross into the runtime (`load_disc_spans`) and are let go. A worker
 * then reads the XA and movie files behind them, so that a movie or a voice
 * clip finds its sectors resident and starts in the frame it asked, as it does
 * with a whole image in memory. A request that beats the worker reads its own
 * span: `span` answers null until then, and the consumer asks again next frame.
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

  /* One read off the main thread; the buffer moves to the page without a copy.
   * Inline: a worker's own URL carries no content stamp (`_gen.py` stamps the
   * pages' references), so a file would go stale in a visitor's cache. */
  const FILL_WORKER = `self.onmessage = async (e) => {
    const { blob, start, end } = e.data;
    try {
      const buffer = await blob.slice(start, end).arrayBuffer();
      self.postMessage({ buffer }, [buffer]);
    } catch (err) {
      self.postMessage({ error: String(err) });
    }
  };`;

  class PlayDisc {
    constructor(blob, layout) {
      this.blob = blob;
      this.layout = layout;
      this.size = blob ? blob.size : 0;
      /* What is resident: `{ start, end, bytes }` in file bytes. */
      this.blocks = [];
      /* The sectors up to PROT.DAT's end, until the runtime has its copy. */
      this.head = null;
      /* The whole image, when it is not one this file can read in parts. */
      this.whole = null;
      this.reads = new Map();
      this.failed = new Set();
      this.worker = null;
      this.closed = false;
    }

    /* The image's first read. `prog` is the page's LoadProgress. */
    static async open(source, prog, text) {
      const blob = blobOf(source);
      let layout = null;
      if (blob) {
        try { layout = await layoutOf(blob); } catch (e) { layout = null; }
      }
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

    /* Read the XA and movie files behind PROT.DAT, off the main thread. The
     * trailing DMY.DAT holds nothing a consumer asks for, and is still there
     * through `prefetch`. A worker that cannot run leaves the on-demand path. */
    startFill() {
      if (!this.layout || this.worker || this.closed) return;
      const start = this.layout.protEnd * RAW;
      const end = this.layout.dmyLba > this.layout.protEnd ? this.layout.dmyLba * RAW : this.size;
      if (start >= end) return;
      const t = performance.now();
      try {
        const url = URL.createObjectURL(new Blob([FILL_WORKER], { type: 'text/javascript' }));
        const w = new Worker(url);
        URL.revokeObjectURL(url);
        this.worker = w;
        w.onmessage = (e) => {
          w.terminate();
          this.worker = null;
          if (this.closed) return;
          if (e.data.error) { console.warn('disc fill', e.data.error); return; }
          this.fillMs = performance.now() - t;
          /* It covers what requests read ahead of it. */
          this.blocks = this.blocks.filter((b) => b.start < start || b.end > end);
          this.blocks.push({ start, end, bytes: new Uint8Array(e.data.buffer) });
        };
        w.onerror = (e) => {
          console.warn('disc fill', e.message);
          w.terminate();
          this.worker = null;
        };
        w.postMessage({ blob: this.blob, start, end });
      } catch (e) {
        console.warn('disc fill', e);
      }
    }

    /* The window `lba` + `sectors` names, clamped to the image as a subarray
     * of the image would be. */
    _extent(lba, sectors) {
      const start = lba * RAW;
      return [start, Math.min(this.size, start + sectors * RAW)];
    }

    /* Those raw sectors if they are resident, else null. An empty array for a
     * window wholly past the image's end, where there is nothing to wait for. */
    span(lba, sectors) {
      const [start, end] = this._extent(lba, sectors);
      if (start >= end) return EMPTY;
      for (const b of this.blocks) {
        if (b.start <= start && end <= b.end) return b.bytes.subarray(start - b.start, end - b.start);
      }
      return null;
    }

    /* Make those sectors resident. The one read of a span settles every asker;
     * a failed span (the file moved, say) is not tried again - `failedSpan`
     * tells the consumer so it can stop waiting. */
    prefetch(lba, sectors) {
      const [start, end] = this._extent(lba, sectors);
      const key = start + ':' + end;
      const known = this.reads.get(key);
      if (known) return known;
      if (start >= end || !this.blob || this.failed.has(key) || this.span(lba, sectors)) return Promise.resolve();
      const read = readBlob(this.blob, start, end).then((bytes) => {
        this.blocks.push({ start, end, bytes });
      }, (e) => {
        console.warn('disc read', start, end, e);
        this.failed.add(key);
      }).finally(() => { this.reads.delete(key); });
      this.reads.set(key, read);
      return read;
    }

    failedSpan(lba, sectors) {
      const [start, end] = this._extent(lba, sectors);
      return this.failed.has(start + ':' + end);
    }

    /* A new disc replaced this one. */
    close() {
      this.closed = true;
      if (this.worker) this.worker.terminate();
      this.worker = null;
      this.blocks = [];
      this.head = null;
    }
  }

  window.LegaiaPlayDisc = { open: PlayDisc.open, RAW: RAW };
}());
