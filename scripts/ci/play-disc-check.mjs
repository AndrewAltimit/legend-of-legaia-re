#!/usr/bin/env node
// The play page's disc reader against a whole read of the same bytes.
//
//   scripts/ci/play-disc-check.mjs [--disc]
//
// Runs site/js/play-disc.js as the page does, over a Blob. The synthetic half needs no disc
// and walks what a reader under a byte cap can get wrong: a held window kept through eviction,
// a read landing with nobody waiting, a narrower read beaten by a wider one, no stitching of
// neighbours, a failed read retried then forgotten, a read in flight surviving eviction. With
// `--disc` and LEGAIA_DISC_BIN set it reads every XA and movie file and a spread of file-prefix
// spans through the reader, under eviction, and compares each span's hash with the same span
// read whole, and again after it was evicted and read back. Prints hashes of nothing: counts
// only, so no output is disc-derived. A `[skip]` for the disc half is not a pass of it.

import assert from 'assert';
import crypto from 'crypto';
import fs from 'fs';
import path from 'path';
import vm from 'vm';
import { fileURLToPath } from 'url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const RAW = 2352, DATA_OFFSET = 24, DATA = 2048;
const MIB = 1048576;

function loadReader() {
  const window = {};
  const warnings = [];
  const console_ = { warn: (...a) => warnings.push(a.join(' ')), log() {}, error() {} };
  vm.runInNewContext(fs.readFileSync(path.join(REPO, 'site/js/play-disc.js'), 'utf8'), {
    window, Blob, setTimeout, Uint8Array, Uint32Array, Math, Map, Set, Promise, String, Error, console: console_,
  });
  return { api: window.LegaiaPlayDisc, warnings };
}

/* An image whose every byte names its place, with a Mode 2/2352 root holding PROT.DAT. */
function syntheticImage(sectors) {
  const img = new Uint8Array(sectors * RAW);
  for (let i = 0; i < img.length; i += 4) new DataView(img.buffer).setUint32(i, i * 2654435761 >>> 0, true);
  const put32 = (o, v) => new DataView(img.buffer).setUint32(o, v, true);
  const pvd = 16 * RAW + DATA_OFFSET;
  img[pvd] = 1;
  img.set(new TextEncoder().encode('CD001'), pvd + 1);
  put32(pvd + 156 + 2, 18);
  put32(pvd + 156 + 10, DATA);
  const root = 18 * RAW + DATA_OFFSET;
  img.fill(0, root, root + DATA);
  const name = 'PROT.DAT;1';
  img[root] = 33 + name.length + 1;
  put32(root + 2, 20);
  put32(root + 10, 4 * DATA);
  img[root + 32] = name.length;
  img.set(new TextEncoder().encode(name), root + 33);
  return img;
}

const sha = (b) => crypto.createHash('sha256').update(b).digest('hex');
const prog = { read: (src) => src.arrayBuffer().then((ab) => new Uint8Array(ab)) };
const tick = (ms = 0) => new Promise((r) => setTimeout(r, ms));

/* A Blob whose reads a test can hold back or fail. */
class Gate {
  constructor(blob) { this.blob = blob; this.size = blob.size; this.held = null; this.failing = 0; this.reads = 0; }
  hold() { this.held = []; }
  free() { const h = this.held; this.held = null; for (const f of h) f(); }
  slice(s, e) {
    const b = this.blob.slice(s, e);
    return { arrayBuffer: async () => {
      this.reads++;
      if (this.held) await new Promise((r) => this.held.push(r));
      if (this.failing > 0) { this.failing--; throw new Error('gate: failed'); }
      return b.arrayBuffer();
    } };
  }
}

async function open(api, img) {
  const blob = new Blob([img]);
  const disc = await api.open(blob, prog, '');
  assert(disc.layout, 'the synthetic image has a layout');
  disc.gate = new Gate(blob);
  disc.blob = disc.gate;
  return disc;
}
const windowOf = (mib) => Math.ceil(mib * MIB / RAW);
const resident = (d) => d.blocks.reduce((a, b) => a + b.end - b.start, 0);

const tests = {
  async 'a window reads whole, every asker shares one read, bytes equal'(api, img) {
    const d = await open(api, img);
    assert.equal(d.span(100, 8), null);
    await Promise.all([d.prefetch(100, 8), d.prefetch(100, 8), d.prefetch(100, 4), d.read(102, 2)]);
    assert.equal(d.gate.reads, 1);
    assert.equal(sha(d.span(100, 8)), sha(img.subarray(100 * RAW, 108 * RAW)));
    assert.equal(sha(d.span(100, 4)), sha(img.subarray(100 * RAW, 104 * RAW)));
  },
  async 'neighbours are not stitched'(api, img) {
    const d = await open(api, img);
    await d.prefetch(100, 4);
    await d.prefetch(104, 4);
    assert.equal(d.span(102, 4), null);
    await d.prefetch(102, 4);
    assert.equal(sha(d.span(102, 4)), sha(img.subarray(102 * RAW, 106 * RAW)));
  },
  async 'past the end is empty, not a wait'(api, img) {
    const d = await open(api, img);
    const past = img.length / RAW + 10;
    assert.equal(d.span(past, 4).length, 0);
    await d.prefetch(past, 4);
    assert.equal((await d.read(past, 4)).length, 0);
    const lastTwo = img.length / RAW - 2;
    await d.prefetch(lastTwo, 8);
    assert.equal(d.span(lastTwo, 8).length, 2 * RAW, 'clamped to the image');
  },
  async 'a released window is evicted under the cap, oldest use first; reread bytes are equal'(api, img) {
    const d = await open(api, img);
    const n = windowOf(12);
    const lbas = [1000, 7000, 13000];
    const want = lbas.map((l) => sha(img.subarray(l * RAW, (l + n) * RAW)));
    for (const l of lbas.slice(0, 2)) { await d.prefetch(l, n); d.release(l, n); }
    assert(d.span(1000, n), 'still resident');   /* now the newest use */
    for (const l of lbas.slice(2)) { await d.prefetch(l, n); d.release(l, n); }
    assert(resident(d) <= api.RESIDENT_CAP);
    assert.equal(d.span(7000, n), null, 'the oldest use went');
    assert(d.span(1000, n), 'the one touched stayed');
    await d.prefetch(7000, n);
    assert.equal(sha(d.span(7000, n)), want[1], 'evicted and read again');
    d.release(7000, n);
  },
  async 'a held window outlives the cap until its release'(api, img) {
    const d = await open(api, img);
    const n = windowOf(20);
    await d.prefetch(1000, n);
    for (const l of [5000, 10000, 15000]) { await d.prefetch(l, n); d.release(l, n); }
    assert(d.span(1000, n), 'held through three landings');
    assert(resident(d) <= api.RESIDENT_CAP + n * RAW);
    d.release(1000, n);
    assert(resident(d) <= api.RESIDENT_CAP);
  },
  async 'a window larger than the cap installs, and drop lets it go'(api, img) {
    const d = await open(api, img);
    const n = windowOf(33);
    await d.prefetch(1000, n);
    assert(d.span(1000, n));
    d.release(1000, n, true);
    assert.equal(d.span(1000, n), null);
    assert.equal(d.blocks.length, 0);
  },
  async 'a read in flight is safe from eviction: read() answers with its bytes after the block is gone'(api, img) {
    const d = await open(api, img);
    d.gate.hold();
    const p = d.read(1000, 8);
    const q = d.prefetch(1000, 8);
    d.release(1000, 8);
    d.gate.free();
    await q;
    assert.equal(sha(await p), sha(img.subarray(1000 * RAW, 1008 * RAW)));
    assert.equal(d.blocks.length, 0, 'nobody holds it: not kept');
  },
  async 'a read landing with nobody waiting is dropped (a movie past its timeout)'(api, img) {
    const d = await open(api, img);
    d.gate.hold();
    const p = d.prefetch(1000, 8);
    d.release(1000, 8, true);
    d.gate.free();
    await p;
    assert.equal(d.blocks.length, 0);
    assert.equal(d.reads.size, 0);
  },
  async 'a wider read and a narrower one leave one block, whichever lands first'(api, img) {
    for (const widerFirst of [false, true]) {
      const d = await open(api, img);
      const first = widerFirst ? 16 : 8, second = widerFirst ? 8 : 16;
      await d.prefetch(1000, first);
      await d.prefetch(1000, second);
      assert.equal(d.blocks.length, 1);
      assert.equal(d.blocks[0].end - d.blocks[0].start, 16 * RAW);
      assert.equal(sha(d.span(1000, 8)), sha(img.subarray(1000 * RAW, 1008 * RAW)));
    }
    /* Both in flight, the narrower late: its bytes are already covered. */
    const d = await open(api, img);
    const gates = [];
    const real = d.blob;
    d.blob = { slice: (s, e) => ({ arrayBuffer: async () => { await new Promise((r) => gates.push(r)); return real.blob.slice(s, e).arrayBuffer(); } }) };
    const narrow = d.prefetch(1000, 8);
    const wide = d.prefetch(1000, 16);
    await tick();
    gates[1]();
    await wide;
    gates[0]();
    await narrow;
    assert.equal(d.blocks.length, 1);
    assert.equal(d.blocks[0].end - d.blocks[0].start, 16 * RAW);
  },
  async 'a failed read is retried, then reported, then forgotten on release'(api, img) {
    const d = await open(api, img);
    d.gate.failing = 2;
    await d.prefetch(1000, 8);
    assert(d.span(1000, 8), 'two failures and a good read');
    assert.equal(d.gate.reads, 3);
    d.gate.failing = 100;
    await d.prefetch(2000, 8);
    assert.equal(d.span(2000, 8), null);
    assert(d.failedSpan(2000, 8));
    const reads = d.gate.reads;
    await d.prefetch(2000, 8);
    assert.equal(d.gate.reads, reads, 'not tried again while the consumer is told');
    d.release(2000, 8);
    assert(!d.failedSpan(2000, 8));
    d.gate.failing = 0;
    await d.prefetch(2000, 8);
    assert.equal(sha(d.span(2000, 8)), sha(img.subarray(2000 * RAW, 2008 * RAW)), 'the next ask reads again');
    d.gate.failing = 100;
    assert.equal(await d.read(3000, 8), null, 'read() answers null');
  },
  async 'the head is read again for a rebuild, whatever was evicted'(api, img) {
    const d = await open(api, img);
    const n = windowOf(20);
    for (const l of [5000, 10000, 15000]) { await d.prefetch(l, n); d.release(l, n); }
    const rt = {
      disc_image_missing: () => new Uint32Array(0),
      load_disc_spans: (size, lbas, blocks) => { rt.head = blocks[0]; return 1; },
    };
    d.head = null;
    await d.loadInto(rt);
    assert.equal(sha(rt.head), sha(img.subarray(0, 24 * RAW)));
  },
  async 'an image with no layout stays whole, and is never evicted'(api, img) {
    const bare = img.subarray(0, 40 * RAW + 7);
    const d = await api.open(new Blob([bare]), prog, '');
    assert.equal(d.layout, null);
    assert.equal(d.whole.length, bare.length);
    assert.equal(sha(d.span(10, 4)), sha(bare.subarray(10 * RAW, 14 * RAW)));
    d.release(10, 4, true);
    d._evict();
    assert(d.span(10, 4));
  },
  async 'a closed disc lands nothing'(api, img) {
    const d = await open(api, img);
    d.gate.hold();
    const p = d.prefetch(1000, 8);
    d.close();
    d.gate.free();
    await p;
    assert.equal(d.blocks.length, 0);
  },
};

function isoFiles(fd) {
  const sector = (lba, n = 1) => {
    const out = Buffer.alloc(n * DATA), s = Buffer.alloc(RAW);
    for (let i = 0; i < n; i++) { fs.readSync(fd, s, 0, RAW, (lba + i) * RAW); s.copy(out, i * DATA, DATA_OFFSET, DATA_OFFSET + DATA); }
    return out;
  };
  const walk = (lba, size, pre) => {
    const d = sector(lba, Math.ceil(size / DATA));
    const files = [];
    for (let o = 0; o < d.length;) {
      const len = d[o];
      if (!len) { o = (Math.floor(o / DATA) + 1) * DATA; continue; }
      const name = d.toString('latin1', o + 33, o + 33 + d[o + 32]).split(';')[0];
      if (d[o + 32] !== 1 || d[o + 33] > 1) {
        if (d[o + 25] & 2) files.push(...walk(d.readUInt32LE(o + 2), d.readUInt32LE(o + 10), pre + name + '/'));
        else files.push({ path: pre + name, lba: d.readUInt32LE(o + 2), sectors: Math.ceil(d.readUInt32LE(o + 10) / DATA) });
      }
      o += len;
    }
    return files;
  };
  const root = sector(16).subarray(156);
  return walk(root.readUInt32LE(2), root.readUInt32LE(10), '');
}

async function discHalf(api) {
  const file = process.env.LEGAIA_DISC_BIN;
  if (!process.argv.includes('--disc')) return console.log('[skip] disc half: pass --disc');
  if (!file || !fs.existsSync(file)) return console.log('[skip] disc half: LEGAIA_DISC_BIN unset');
  const fd = fs.openSync(file, 'r');
  const files = isoFiles(fd).filter((f) => /\.(XA|STR)$/.test(f.path));
  assert(files.length >= 40, 'the retail image has 34 XA files and 6 movies');
  const whole = (lba, n) => {
    const b = Buffer.alloc(n * RAW);
    fs.readSync(fd, b, 0, b.length, lba * RAW);
    return sha(b);
  };
  const disc = await api.open(await fs.openAsBlob(file), prog, '');
  assert(disc.layout, 'the disc has a layout');
  /* Every file whole, then two prefixes of it, as clip spans are; held, served, let go. */
  const spans = files.flatMap((f) => [f.sectors, Math.ceil(f.sectors / 2), Math.ceil(f.sectors / 8)].map((n) => ({ lba: f.lba, n, movie: /STR$/.test(f.path) })));
  let served = 0, peak = 0;
  const first = new Map();
  for (const s of spans) {
    const want = whole(s.lba, s.n);
    first.set(s, want);
    await disc.prefetch(s.lba, s.n);
    assert.equal(sha(disc.span(s.lba, s.n)), want, `span ${s.lba}+${s.n}`);
    peak = Math.max(peak, resident(disc));
    disc.release(s.lba, s.n, s.movie);
    assert(resident(disc) <= api.RESIDENT_CAP, 'over the cap once released');
    served++;
  }
  /* Whatever eviction left: read them all once more, in reverse, through both paths. */
  let reread = 0, wasGone = 0;
  for (const s of spans.slice().reverse()) {
    if (!disc.span(s.lba, s.n)) wasGone++;
    assert.equal(sha(await disc.read(s.lba, s.n)), first.get(s), `read ${s.lba}+${s.n}`);
    await disc.prefetch(s.lba, s.n);
    assert.equal(sha(disc.span(s.lba, s.n)), first.get(s), `reread ${s.lba}+${s.n}`);
    disc.release(s.lba, s.n, s.movie);
    reread++;
  }
  fs.closeSync(fd);
  console.log(`[ok] disc: ${served} spans equal a whole read, ${reread} again, ${wasGone} of them after eviction; peak resident ${(peak / MIB).toFixed(1)} MiB, cap ${(api.RESIDENT_CAP / MIB).toFixed(0)}`);
}

const { api, warnings } = loadReader();
const img = syntheticImage(25000);
let failed = 0;
for (const [name, t] of Object.entries(tests)) {
  try { await t(api, img); console.log(`[ok] ${name}`); }
  catch (e) { failed++; console.log(`[FAIL] ${name}: ${e.message}`); }
}
if (!failed) await discHalf(api);
void warnings;
process.exit(failed ? 1 : 0);
