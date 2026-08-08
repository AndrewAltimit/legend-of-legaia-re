/* disc-info.js - disc-identity panel for the user-supplied image.
 *
 * When a page receives a disc (fresh pick or the RomCache auto-load), this
 * module reads JUST the sectors it needs - never the whole file - and renders
 * a card next to the file input identifying what the user actually loaded:
 *
 *   - product code / serial (SYSTEM.CNF `BOOT =` line) + the known-build row
 *     it matches (region, language, release date - docs/reference/builds.md)
 *   - executable name + its ISO recording timestamp (the build's master date)
 *   - ISO9660 volume metadata (volume id, publisher, mastering date)
 *   - the Sony region mark (PS-X EXE header string, else system-area scan)
 *   - image geometry (sector mode, sector count, trailing-byte warning)
 *   - PROT.DAT entry count vs the 1233 every retail region ships
 *   - on-demand SHA-1 / SHA-256 fingerprints (redump databases key on SHA-1)
 *   - cover art, hotlinked at runtime from the libretro-thumbnails project
 *     keyed by the disc's OWN serial - nothing is bundled with the site, and
 *     the request carries only a static URL, never any byte of the disc.
 *
 * All reads go through Blob.slice, so showing the card costs a few KB of I/O
 * even for a 700 MB image and works before (or without) the page's own
 * full-file load. Accepts Mode2/2352 .bin images, plain 2048-byte ISOs
 * (identified, but flagged - the site's loaders want the raw .bin), and bare
 * PROT.DAT archives (no ISO layer to identify; entry count only).
 *
 * Wiring: rom-cache.js calls `window.DiscInfo.onDisc(source, input)` when a
 * disc arrives; pages with their own <input> (the ROM patcher) call it from
 * their change handler. Pages that don't include this file are unchanged.
 *
 * The parsing core is DOM-free and exported for Node so a disc-gated test
 * can drive it against a real image (see scripts/ci or scratchpad harness):
 *   identify(reader) where reader = { size, name, read(offset, len) }.
 */
(function () {
  'use strict';

  var RAW = 2352; // Mode2 raw sector
  var DATA = 2048; // user-data payload
  var RAW_DATA_OFF = 24; // sync(12) + header(4) + subheader(8) -> Form1 data

  // ---------------------------------------------------------------------
  // Known builds - distilled from docs/reference/builds.md (project research,
  // not disc bytes). `cover` is the libretro-thumbnails redump name.
  // ---------------------------------------------------------------------
  var KNOWN_BUILDS = {
    'SCUS-94254': {
      title: 'Legend of Legaia', region: 'NTSC-U', locale: 'USA, English',
      kind: 'NA retail', date: '1999-01-29', cover: 'Legend of Legaia (USA)',
      note: 'anchor build for this project',
    },
    'SCES-01752': {
      title: 'Legend of Legaia', region: 'PAL', locale: 'Europe, English',
      kind: 'EU retail', date: '1999-09-27', cover: 'Legend of Legaia (Europe)',
    },
    'SCES-01944': {
      title: 'Legend of Legaia', region: 'PAL', locale: 'France, French',
      kind: 'EU retail', date: '2000-04-05', cover: 'Legend of Legaia (France)',
    },
    'SCES-01945': {
      title: 'Legend of Legaia', region: 'PAL', locale: 'Germany, German',
      kind: 'EU retail', date: '2000-05-10', cover: 'Legend of Legaia (Germany)',
    },
    'SCES-01946': {
      title: 'Legend of Legaia', region: 'PAL', locale: 'Italy, Italian',
      kind: 'EU retail', date: '2000-04-26', cover: 'Legend of Legaia (Italy)',
    },
    'SCES-01947': {
      title: 'Legend of Legaia', region: 'PAL', locale: 'Spain, Spanish',
      kind: 'EU retail', date: '2000', cover: 'Legend of Legaia (Spain)',
    },
    'SCPS-10059': {
      title: 'Legaia Densetsu', region: 'NTSC-J', locale: 'Japan, Japanese',
      kind: 'JP retail', date: '1998-09-09', cover: 'Legaia Densetsu (Japan)',
      note: 'a 1998-11-16 build under this serial is the NA prototype',
    },
    'SCPS-91246': {
      title: 'Legaia Densetsu', region: 'NTSC-J', locale: 'Japan, Japanese',
      kind: 'JP "PlayStation the Best" reissue', cover: 'Legaia Densetsu (Japan)',
    },
    'SCPS-45340': {
      title: 'Legaia Densetsu', region: 'NTSC-J', locale: 'Japan, Japanese',
      kind: 'JP variant (undocumented)', cover: 'Legaia Densetsu (Japan)',
    },
    'SCUS-94366': {
      title: 'Legend of Legaia (demo)', region: 'NTSC-U', locale: 'USA, English',
      kind: 'NA demo', date: '1998-12-21',
    },
    'PAPX-90040': {
      title: 'Legaia trial (PrePre Vol. 14)', region: 'NTSC-J',
      locale: 'Japan, Japanese', kind: 'JP trial', date: '1996-06-02',
    },
    'PAPX-90055': {
      title: 'Legaia Densetsu (demo 2)', region: 'NTSC-J',
      locale: 'Japan, Japanese', kind: 'JP demo', date: '1998-08-19',
    },
    'PCPX-96130': {
      title: 'Legaia Densetsu (demo 1)', region: 'NTSC-J',
      locale: 'Japan, Japanese', kind: 'JP demo / kiosk', date: '1998-08-18',
    },
  };

  var COVER_BASE =
    'https://raw.githubusercontent.com/libretro-thumbnails/Sony_-_PlayStation' +
    '/master/Named_Boxarts/';

  var RETAIL_PROT_ENTRIES = 1233;

  /* PROT.DAT header word[1] counts in-RAM TOC slots, which sit +2 ahead of
   * the extraction index space (docs/formats/cdname.md#numbering-space) -
   * retail's 1233 entries are stored as 1235. */
  function protFromHeaderWord(word, sizeBytes) {
    var entries = word - 2;
    return {
      entries: entries,
      sizeBytes: sizeBytes,
      matchesRetail: entries === RETAIL_PROT_ENTRIES,
    };
  }

  // ---------------------------------------------------------------------
  // Byte helpers (DOM-free)
  // ---------------------------------------------------------------------
  function u32le(b, o) {
    return (b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24)) >>> 0;
  }
  function ascii(b, start, end) {
    var s = '';
    for (var i = start; i < end && i < b.length; i++) {
      var c = b[i];
      if (c === 0) break;
      s += c >= 0x20 && c < 0x7f ? String.fromCharCode(c) : ' ';
    }
    return s;
  }
  function tidy(s) { return s.replace(/\s+/g, ' ').trim(); }
  function pad2(n) { return (n < 10 ? '0' : '') + n; }

  /* "SCUS_942.54;1" / "SCES_019.44" -> "SCUS-94254" */
  function serialFromExeName(name) {
    var m = /^([A-Z]{4})[_-]?(\d{3})\.?(\d{2})/i.exec(name.replace(/;\d+$/, ''));
    if (!m) return null;
    return (m[1] + '-' + m[2] + m[3]).toUpperCase();
  }

  /* ISO9660 dir-record recording date (7 bytes) -> "1999-01-13 04:41" */
  function recDate(b, o) {
    if (b[o] === 0 && b[o + 1] === 0) return null;
    return (1900 + b[o]) + '-' + pad2(b[o + 1]) + '-' + pad2(b[o + 2]) +
      ' ' + pad2(b[o + 3]) + ':' + pad2(b[o + 4]);
  }

  /* PVD dec-datetime "YYYYMMDDHHMMSS.." -> "1999-01-14 04:11". A zeroed
   * year is real on some PAL masters (France writes 0000-04-04) - treat it
   * as absent rather than display nonsense. */
  function pvdDate(s) {
    if (!/^\d{8}/.test(s) || s.slice(0, 4) === '0000') return null;
    return s.slice(0, 4) + '-' + s.slice(4, 6) + '-' + s.slice(6, 8) +
      ' ' + s.slice(8, 10) + ':' + s.slice(10, 12);
  }

  // ---------------------------------------------------------------------
  // Identification core. `reader` = { size, name, read(offset, len) ->
  // Promise<Uint8Array> } - the browser wraps a Blob, tests wrap a file.
  // ---------------------------------------------------------------------

  function detectMode(head, reader) {
    // Mode2/2352 raw: 12-byte sync pattern 00 FF x10 00 at sector start.
    var sync = head.length >= 12 && head[0] === 0 && head[11] === 0;
    for (var i = 1; sync && i < 11; i++) if (head[i] !== 0xff) sync = false;
    if (sync) return Promise.resolve('bin2352');
    // Plain 2048-byte ISO: "CD001" at sector 16 + 1.
    return reader.read(16 * DATA, 6).then(function (b) {
      if (ascii(b, 1, 6) === 'CD001') return 'iso2048';
      // Bare PROT.DAT: [pad][file_count-1][header_sectors] at 0 or 0x800.
      var count = u32le(head, 4), sect = u32le(head, 8);
      if (count > 0 && count < 0x8000 && sect > 0 && sect < 0x100) return 'prot0';
      if (head.length >= 0x80c) {
        count = u32le(head, 0x804); sect = u32le(head, 0x808);
        if (count > 0 && count < 0x8000 && sect > 0 && sect < 0x100) return 'prot800';
      }
      return 'unknown';
    });
  }

  function makeSectorReader(reader, mode) {
    var rawLen = mode === 'bin2352' ? RAW : DATA;
    var dataOff = mode === 'bin2352' ? RAW_DATA_OFF : 0;
    return function readSectors(lba, count) {
      // Sectors are contiguous on disc but the payload isn't in a raw image;
      // read the raw span once and splice the Form1 windows out.
      return reader.read(lba * rawLen, count * rawLen).then(function (raw) {
        if (rawLen === DATA) return raw;
        var out = new Uint8Array(count * DATA);
        for (var i = 0; i < count; i++) {
          out.set(raw.subarray(i * rawLen + dataOff, i * rawLen + dataOff + DATA), i * DATA);
        }
        return out;
      });
    };
  }

  function parseDirRecords(buf) {
    var files = [];
    var pos = 0;
    while (pos < buf.length) {
      var len = buf[pos];
      if (len === 0) { // records never span sectors; hop to the next one
        pos = (Math.floor(pos / DATA) + 1) * DATA;
        continue;
      }
      var nameLen = buf[pos + 32];
      var name = ascii(buf, pos + 33, pos + 33 + nameLen);
      files.push({
        name: name.replace(/;\d+$/, ''),
        lba: u32le(buf, pos + 2),
        size: u32le(buf, pos + 10),
        date: recDate(buf, pos + 18),
        dir: (buf[pos + 25] & 2) !== 0,
      });
      pos += len;
    }
    return files;
  }

  function findFile(files, name) {
    name = name.toUpperCase();
    for (var i = 0; i < files.length; i++) {
      if (!files[i].dir && files[i].name.toUpperCase() === name) return files[i];
    }
    return null;
  }

  /* Printable runs in the system area (sectors 0..15) that mention Sony -
   * the license/region text mastered into every retail PSX disc. Unlike
   * ascii() this maps NULs to spaces instead of stopping (the area is
   * mostly zero fill with text islands). */
  function scanLicense(raw) {
    var s = '';
    for (var j = 0; j < raw.length; j++) {
      var c = raw[j];
      s += c >= 0x20 && c < 0x7f ? String.fromCharCode(c) : ' ';
    }
    var runs = s.split(/ {4,}/);
    var hits = [];
    for (var i = 0; i < runs.length; i++) {
      var t = tidy(runs[i]);
      if (t.length >= 12 && /Sony|Licensed/i.test(t) && hits.indexOf(t) < 0) hits.push(t);
    }
    return hits.length ? hits.join(' ') : null;
  }

  function identify(reader) {
    var info = {
      fileName: reader.name || '',
      fileSize: reader.size,
      mode: null,
      warnings: [],
      serial: null,
      build: null,
      exe: null, // { name, date, size, regionMark }
      volume: null, // { id, publisher, preparer, created, sectors }
      license: null,
      prot: null, // { entries, sizeBytes, matchesRetail }
      sectorCount: null,
      files: null, // root-file count
    };

    return reader.read(0, Math.min(reader.size, 0x80c + 4)).then(function (head) {
      return detectMode(head, reader);
    }).then(function (mode) {
      info.mode = mode;

      if (mode === 'prot0' || mode === 'prot800') {
        // Bare archive: no ISO layer, no serial - identify by TOC shape.
        var off = mode === 'prot800' ? 0x800 : 0;
        return reader.read(off, 12).then(function (h) {
          info.prot = protFromHeaderWord(u32le(h, 4), reader.size);
          return info;
        });
      }
      if (mode === 'unknown') {
        info.warnings.push('Not a Mode2/2352 .bin, ISO, or PROT.DAT - could not identify.');
        return info;
      }

      var rawLen = mode === 'bin2352' ? RAW : DATA;
      info.sectorCount = Math.floor(reader.size / rawLen);
      if (reader.size % rawLen !== 0) {
        info.warnings.push('Image size is not a whole number of ' + rawLen +
          '-byte sectors (' + (reader.size % rawLen) + ' trailing bytes) - possibly a bad dump.');
      }
      if (mode === 'iso2048') {
        info.warnings.push('2048-byte ISO: identifiable, but the site\'s loaders ' +
          'need the raw Mode2/2352 .bin (XA audio and the raw-sector layer are missing here).');
      }

      var readSectors = makeSectorReader(reader, mode);

      var licenseScan = mode === 'bin2352'
        ? reader.read(0, 16 * RAW).then(function (raw) { info.license = scanLicense(raw); })
        : Promise.resolve();

      return licenseScan.then(function () {
        return readSectors(16, 1);
      }).then(function (pvd) {
        if (ascii(pvd, 1, 6) !== 'CD001') {
          info.warnings.push('No ISO9660 volume descriptor at sector 16.');
          return info;
        }
        info.volume = {
          id: tidy(ascii(pvd, 40, 72)),
          publisher: tidy(ascii(pvd, 318, 446)),
          preparer: tidy(ascii(pvd, 446, 574)),
          created: pvdDate(ascii(pvd, 813, 830)),
          sectors: u32le(pvd, 80),
        };
        if (info.sectorCount !== null && info.sectorCount < info.volume.sectors) {
          info.warnings.push('Image holds ' + info.sectorCount + ' sectors but the volume ' +
            'declares ' + info.volume.sectors + ' - truncated dump.');
        }

        var rootLba = u32le(pvd, 156 + 2);
        var rootSize = u32le(pvd, 156 + 10);
        var rootSectors = Math.max(1, Math.min(16, Math.ceil(rootSize / DATA)));
        return readSectors(rootLba, rootSectors).then(function (rootBuf) {
          var files = parseDirRecords(rootBuf.subarray(0, rootSize));
          info.files = files.length;
          var chain = [];

          var sysCnf = findFile(files, 'SYSTEM.CNF');
          if (sysCnf) {
            chain.push(readSectors(sysCnf.lba, 1).then(function (b) {
              var m = /BOOT\s*=\s*cdrom:\\?([^\s;]+)/i.exec(ascii(b, 0, sysCnf.size || DATA));
              if (!m) return;
              var exeName = m[1].replace(/^.*\\/, '');
              info.serial = serialFromExeName(exeName);
              var exeEnt = findFile(files, exeName);
              info.exe = {
                name: exeName,
                date: exeEnt ? exeEnt.date : null,
                size: exeEnt ? exeEnt.size : null,
                regionMark: null,
              };
              if (exeEnt) {
                // PS-X EXE header: region-mark string at +0x4C.
                return readSectors(exeEnt.lba, 1).then(function (hdr) {
                  if (ascii(hdr, 0, 8) === 'PS-X EXE') {
                    info.exe.regionMark = tidy(ascii(hdr, 0x4c, 0x8c)) || null;
                  }
                });
              }
            }));
          } else {
            info.warnings.push('No SYSTEM.CNF in the root directory - not a bootable PSX disc?');
          }

          var prot = findFile(files, 'PROT.DAT');
          if (prot) {
            chain.push(readSectors(prot.lba, 1).then(function (h) {
              var word = u32le(h, 4);
              var next = word > 0 && word < 0x8000 ? Promise.resolve(word)
                : readSectors(prot.lba + 1, 1).then(function (h2) { return u32le(h2, 4); });
              return next.then(function (c) {
                info.prot = protFromHeaderWord(c, prot.size);
              });
            }));
          } else {
            info.warnings.push('No PROT.DAT - this disc is not Legend of Legaia.');
          }

          return Promise.all(chain).then(function () {
            if (info.serial) {
              info.build = KNOWN_BUILDS[info.serial] || null;
              if (!info.build) {
                info.warnings.push('Serial ' + info.serial + ' is not in the known-build table.');
              }
            }
            return info;
          });
        });
      });
    });
  }

  // ---------------------------------------------------------------------
  // Browser layer: reader over a Blob, panel rendering, RomCache hook.
  // ---------------------------------------------------------------------
  if (typeof window === 'undefined') {
    module.exports = {
      identify: identify,
      serialFromExeName: serialFromExeName,
      KNOWN_BUILDS: KNOWN_BUILDS,
    };
    return;
  }

  function readerFromSource(src) {
    var blob = src instanceof Blob ? src : src.blob || null;
    if (blob) {
      return {
        name: src.name, size: src.size,
        read: function (off, len) {
          return blob.slice(off, off + len).arrayBuffer().then(function (ab) {
            return new Uint8Array(ab);
          });
        },
      };
    }
    // No Blob handle (unusual): fall back to one full read, then slice.
    var whole = null;
    return {
      name: src.name, size: src.size,
      read: function (off, len) {
        whole = whole || src.arrayBuffer();
        return whole.then(function (ab) {
          return new Uint8Array(ab, off, Math.min(len, ab.byteLength - off));
        });
      },
    };
  }

  function ensureStyle() {
    if (document.getElementById('disc-info-style')) return;
    var s = document.createElement('style');
    s.id = 'disc-info-style';
    s.textContent = [
      /* The card must never widen its host: width comes from the container,
       * long mono values (hashes, region marks) wrap via min-width:0 +
       * overflow-wrap on the value cells, one field per line. */
      '.disc-info-card{display:block;box-sizing:border-box;width:100%;max-width:46rem;',
      'min-width:0;margin:.75rem 0;padding:.8rem 1rem .7rem;',
      'border:1px solid var(--border,#272c37);border-left:3px solid var(--ok,#46c98d);',
      'border-radius:10px;background:var(--bg-card,#1a1e26);text-align:left;}',
      '.disc-info-head{display:flex;flex-wrap:wrap;align-items:center;gap:.5rem;min-width:0;}',
      '.disc-info-dot{flex:none;width:.55rem;height:.55rem;border-radius:50%;',
      'background:var(--ok,#46c98d);box-shadow:0 0 0 0 rgba(70,201,141,.45);',
      'animation:disc-info-pulse 2.4s ease-out infinite;}',
      '@keyframes disc-info-pulse{0%{box-shadow:0 0 0 0 rgba(70,201,141,.45)}',
      '70%{box-shadow:0 0 0 7px rgba(70,201,141,0)}100%{box-shadow:0 0 0 0 rgba(70,201,141,0)}}',
      '@media (prefers-reduced-motion:reduce){.disc-info-dot{animation:none;}}',
      '.disc-info-title{font-weight:650;color:var(--text-bright,#e8ebf1);',
      'overflow-wrap:anywhere;min-width:0;}',
      '.disc-info-badge{flex:none;font-family:var(--font-mono,monospace);font-size:.72rem;',
      'padding:.08rem .45rem;border-radius:5px;border:1px solid var(--accent-line,rgba(61,132,255,.38));',
      'background:var(--accent-soft,rgba(61,132,255,.13));color:var(--accent-hi,#8fb8ff);}',
      '.disc-info-badge.is-region{border-color:rgba(70,201,141,.4);',
      'background:var(--ok-soft,rgba(70,201,141,.12));color:var(--ok,#46c98d);}',
      '.disc-info-sub{color:var(--text-muted,#8d95a5);font-size:.82rem;',
      'margin:.25rem 0 .6rem;overflow-wrap:anywhere;}',
      '.disc-info-flex{display:flex;gap:.9rem;align-items:flex-start;min-width:0;}',
      '.disc-info-cover{flex:none;width:76px;border-radius:6px;',
      'border:1px solid var(--border-hi,#333a48);display:none;}',
      '.disc-info-rows{flex:1;min-width:0;display:flex;flex-direction:column;gap:.28rem;}',
      '.disc-info-row{display:flex;gap:.7rem;align-items:baseline;min-width:0;}',
      '.disc-info-row .k{flex:0 0 6.4rem;font-size:.68rem;font-weight:600;',
      'letter-spacing:.06em;text-transform:uppercase;color:var(--text-dim,#6b7383);}',
      '.disc-info-row .v{flex:1;min-width:0;font-family:var(--font-mono,monospace);',
      'font-size:.78rem;color:var(--text,#c6cdd9);overflow-wrap:anywhere;}',
      '.disc-info-warn{margin:.55rem 0 0;padding:.35rem .6rem;border-radius:6px;',
      'font-size:.78rem;background:var(--warn-soft,rgba(217,168,98,.12));',
      'color:var(--warn,#d9a862);overflow-wrap:anywhere;}',
      '.disc-info-hash button{cursor:pointer;font:inherit;color:var(--accent,#6ba3ff);',
      'background:transparent;border:1px solid var(--border,#272c37);border-radius:5px;',
      'padding:.05rem .5rem;}',
      '.disc-info-hash button:hover{border-color:var(--accent-line,rgba(61,132,255,.38));}',
      '@media (max-width:560px){.disc-info-flex{flex-direction:column;}',
      '.disc-info-cover{width:104px;}',
      '.disc-info-row{flex-direction:column;gap:.05rem;}.disc-info-row .k{flex:none;}}',
    ].join('');
    document.head.appendChild(s);
  }

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }
  function row(rows, label, value) {
    if (value == null || value === '') return null;
    var r = el('div', 'disc-info-row');
    r.appendChild(el('span', 'k', label));
    var v = el('span', 'v', typeof value === 'string' ? value : null);
    r.appendChild(v);
    rows.appendChild(r);
    return v;
  }
  function fmtMB(n) { return (n / 1024 / 1024).toFixed(1) + ' MB'; }

  function coverInto(img, build) {
    if (!build || !build.cover) return;
    img.alt = build.cover + ' box art';
    img.title = 'Cover art fetched from the libretro-thumbnails project';
    /* No loading=lazy: the img starts display:none (shown only once it
     * decodes), and lazy-loading never fires for a boxless image. */
    img.onload = function () { img.style.display = 'block'; };
    img.onerror = function () { img.style.display = 'none'; };
    img.src = COVER_BASE + encodeURIComponent(build.cover) + '.png';
  }

  function hashRow(rows, source) {
    var r = el('div', 'disc-info-row');
    r.appendChild(el('span', 'k', 'Fingerprint'));
    var dd = el('span', 'v disc-info-hash');
    r.appendChild(dd);
    rows.appendChild(r);
    var btn = el('button', null, 'Compute SHA-1 + SHA-256');
    btn.type = 'button';
    btn.title = 'Hashes the whole image locally; redump.org keys dumps by SHA-1';
    btn.addEventListener('click', function () {
      btn.disabled = true;
      btn.textContent = 'Hashing ' + fmtMB(source.size) + ' ...';
      Promise.resolve(source.arrayBuffer()).then(function (ab) {
        return Promise.all([
          crypto.subtle.digest('SHA-1', ab),
          crypto.subtle.digest('SHA-256', ab),
        ]);
      }).then(function (digs) {
        dd.textContent = '';
        ['SHA-1', 'SHA-256'].forEach(function (label, i) {
          var hex = Array.prototype.map.call(new Uint8Array(digs[i]), function (b) {
            return (b < 16 ? '0' : '') + b.toString(16);
          }).join('');
          var line = el('div', null, label + ' ' + hex);
          line.style.userSelect = 'all';
          dd.appendChild(line);
        });
      }).catch(function (e) {
        btn.disabled = false;
        btn.textContent = 'Compute SHA-1 + SHA-256 (failed - retry)';
        console.warn('DiscInfo: hashing failed -', e);
      });
    });
    dd.appendChild(btn);
  }

  function render(panel, info, source) {
    panel.textContent = '';
    panel.className = 'disc-info-card';

    var b = info.build;
    var head = el('div', 'disc-info-head');
    var dot = el('span', 'disc-info-dot');
    dot.title = 'Identified from the loaded image';
    head.appendChild(dot);
    head.appendChild(el('span', 'disc-info-title',
      b ? b.title : info.prot && !info.serial ? 'PROT.DAT archive' : 'Unidentified disc image'));
    if (info.serial) head.appendChild(el('span', 'disc-info-badge', info.serial));
    if (b && b.region) head.appendChild(el('span', 'disc-info-badge is-region', b.region));
    panel.appendChild(head);

    if (b) {
      var sub = b.kind + (b.locale ? ' · ' + b.locale : '') +
        (b.date ? ' · released ' + b.date : '') +
        (b.note ? ' · ' + b.note : '');
      panel.appendChild(el('div', 'disc-info-sub', sub));
    }

    var flex = el('div', 'disc-info-flex');
    panel.appendChild(flex);
    var cover = el('img', 'disc-info-cover');
    flex.appendChild(cover);
    coverInto(cover, b);
    var rows = el('div', 'disc-info-rows');
    flex.appendChild(rows);

    if (info.exe) {
      row(rows, 'Executable',
        info.exe.name + (info.exe.date ? ' · mastered ' + info.exe.date : ''));
    }
    if (info.volume) {
      var vparts = [];
      if (info.volume.id) vparts.push('"' + info.volume.id + '"');
      if (info.volume.created) vparts.push('built ' + info.volume.created);
      if (info.volume.publisher) vparts.push(info.volume.publisher);
      row(rows, 'Volume', vparts.join(' · '));
    }
    /* Prefer the system-area license text - it is what the console's BIOS
     * actually validates. The PS-X EXE header string is only a fallback:
     * the PAL Legaia masters carry the NORTH AMERICA region-mark string in
     * their exe while the system area correctly says Europe. */
    var mark = info.license || (info.exe && info.exe.regionMark);
    row(rows, 'Region mark', mark);
    if (info.mode === 'bin2352' || info.mode === 'iso2048') {
      row(rows, 'Image',
        (info.mode === 'bin2352' ? 'Mode2/2352 .bin' : '2048-byte ISO') +
        ' · ' + (info.sectorCount || 0).toLocaleString() + ' sectors · ' +
        fmtMB(info.fileSize));
    } else if (info.prot) {
      row(rows, 'Image', 'bare PROT.DAT · ' + fmtMB(info.fileSize));
    }
    if (info.prot) {
      row(rows, 'PROT.DAT', info.prot.entries.toLocaleString() + ' entries · ' +
        fmtMB(info.prot.sizeBytes) +
        (info.prot.matchesRetail ? ' · matches retail layout'
          : ' · does NOT match the retail 1233-entry layout'));
    }
    hashRow(rows, source);

    for (var i = 0; i < info.warnings.length; i++) {
      panel.appendChild(el('div', 'disc-info-warn', info.warnings[i]));
    }
  }

  // One panel per anchor element, keyed to the delivered disc so a same-disc
  // re-pick doesn't re-run the reads.
  var panels = []; // { anchor, panel, key }

  function panelFor(anchor, placeAfter) {
    for (var i = 0; i < panels.length; i++) {
      if (panels[i].anchor === anchor) return panels[i];
    }
    ensureStyle();
    var panel = document.createElement('div');
    panel.style.display = 'none';
    placeAfter.insertAdjacentElement('afterend', panel);
    var entry = { anchor: anchor, panel: panel, key: null };
    panels.push(entry);
    return entry;
  }

  /* Where an <input>'s panel sits: under the RomCache chip when there is
   * one, else under the input's control group. */
  function inputPlacement(input) {
    var group = (input.closest && input.closest('.file-input-group')) || input;
    var next = group.nextElementSibling;
    if (next && next.classList && next.classList.contains('rom-cache-chip')) return next;
    return group;
  }

  function renderAt(anchor, placeAfter, source) {
    var entry = panelFor(anchor, placeAfter);
    var key = (source.name || '') + ' ' + source.size;
    if (entry.key === key) return;
    entry.key = key;
    identify(readerFromSource(source)).then(function (info) {
      if (entry.key !== key) return; // a different disc arrived meanwhile
      entry.panel.style.display = '';
      render(entry.panel, info, source);
    }).catch(function (e) {
      console.warn('DiscInfo: identify failed -', e);
      if (entry.key === key) entry.key = null;
    });
  }

  function onDisc(source, input) {
    if (!source || !input) return;
    renderAt(input, inputPlacement(input), source);
  }

  /* For pages with their own <input> (no RomCache): wire change directly. */
  function attachInput(input) {
    if (!input) return;
    input.addEventListener('change', function () {
      var f = input.files && input.files[0];
      if (f) onDisc(f, input);
    });
  }

  /* Read the shared rom-cache record without requiring rom-cache.js (the
   * same store layout.js peeks at) - full record, blob included. */
  function readCachedRecord() {
    if (window.RomCache && window.RomCache.get) return window.RomCache.get();
    return new Promise(function (resolve) {
      if (typeof indexedDB === 'undefined') return resolve(null);
      var req;
      try { req = indexedDB.open('legaia-rom-cache', 1); } catch (e) { return resolve(null); }
      req.onupgradeneeded = function () {
        var db = req.result;
        if (!db.objectStoreNames.contains('disc')) db.createObjectStore('disc');
      };
      req.onerror = function () { resolve(null); };
      req.onsuccess = function () {
        var db = req.result;
        try {
          var get = db.transaction('disc', 'readonly').objectStore('disc').get('current');
          get.onsuccess = function () { db.close(); resolve(get.result || null); };
          get.onerror = function () { db.close(); resolve(null); };
        } catch (e) { db.close(); resolve(null); }
      };
    });
  }

  /* Render the cached disc's identity under `anchor` (the home page's disc
   * slot). No-op when nothing is cached. */
  function intoCached(anchor) {
    if (!anchor) return;
    readCachedRecord().then(function (rec) {
      if (!rec || !rec.blob) return;
      renderAt(anchor, anchor, {
        name: rec.name, size: rec.size, blob: rec.blob,
        arrayBuffer: function () { return rec.blob.arrayBuffer(); },
      });
    }).catch(function (e) {
      console.warn('DiscInfo: cached-disc read failed -', e);
    });
  }

  window.DiscInfo = {
    identify: identify,
    onDisc: onDisc,
    attachInput: attachInput,
    intoCached: intoCached,
  };
})();
