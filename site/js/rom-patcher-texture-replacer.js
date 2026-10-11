/* rom-patcher-texture-replacer.js - the ROM patcher's texture replacement UI.
 *
 * Scan grid, filter and presets, the per-texture replace dialog, the queue
 * (persisted between reloads) and change-pack import / export, all over the
 * WASM texture API.
 *
 * Module script, loaded after rom-patcher-page-controls.js and ahead of
 * rom-patcher-app.js; publishes on window.LegaiaRomPatcher. */

const { $ } = window.LegaiaRomPatcher;

// --- Texture replacement ----------------------------------------------------
//
// Client-side texture swap over the WASM texture API. Everything family-shaped
// lives on the Rust side: `scan_textures` returns both the rows and a `tiers`
// list describing each family (id, title, what it is, whether it can be
// written), so this file has no hardcoded knowledge of which texture families
// exist. A new family appears in the grid and in the presets on its own.
//
// Coordinates are the quad `(tier, entry, section, offset)`: entry -1 is the
// unindexed PROT.DAT gap, and `section` means whatever its family says it does
// (an LZS section index, a save slot, a side-band slot) or -1.

// Where the queue is kept between reloads. Versioned with the pack format it
// stores, so a future format change cannot half-read an old blob.
const TEX_QUEUE_KEY = 'legaia-rom-patcher.texture-queue.v1';

// Paint a `{ w, h, rgba }` image onto a canvas at its native size.
function drawRgba(canvas, img) {
  canvas.width = img.w;
  canvas.height = img.h;
  const ctx = canvas.getContext('2d');
  ctx.putImageData(new ImageData(new Uint8ClampedArray(img.rgba), img.w, img.h), 0, 0);
}

// Human-readable coordinate for a scan row / queue item. A family that
// addresses its rows by slot says so; everything else names a byte offset.
function texDesc(t) {
  const off = '0x' + t.offset.toString(16).toUpperCase();
  if (t.tier === 'save-icon') return `save icon · slot ${t.section} (save ${t.section + 1})`;
  // Battle art numbers its blocks over two slot spaces in one signed field:
  // an equipment record index, or a shared header block at -1 - n. Spell
  // both out rather than printing a bare negative section.
  if (t.tier === 'battle-equip') {
    const slot = t.section >= 0
      ? `equipment record ${t.section}`
      : `shared header block ${-1 - t.section}`;
    return `battle art · entry ${t.entry} ${slot} +${off}`;
  }
  // A monster sheet is addressed by the archive slot the monster occupies,
  // and that slot number IS the monster id every other tool prints.
  if (t.tier === 'monster') return `monster skin · id ${t.section} +${off}`;
  const where = t.entry < 0 ? `gap +${off}`
    : t.section >= 0 ? `entry ${t.entry} sec ${t.section} +${off}`
      : `entry ${t.entry} +${off}`;
  return `${t.tier} ${where}`;
}

// Spellings of a label a person is as likely to type as the disc's own. The
// disc writes the armband "Ra-Seru", and someone hunting it types "raseru"
// about as often - so the haystack carries the label twice more, once with
// its punctuation turned into spaces and once with the punctuation simply
// removed (that second copy is what makes "raseru" reach "Ra-Seru").
//
// The `$N` suffix retail puts on an upgradeable weapon ("Ra-Seru Ozma $1") is
// its upgrade tier, and measurably so: within each family the equipment
// table's attack bonus rises strictly with N (Ozma 24, 36, 48, 60, 72, 89,
// 106 across `$1..$7`). So `tier N` is a synonym here, not a guess.
//
// Search vocabulary only. Nothing here reaches the label the cell displays,
// which stays exactly what the disc says.
function texAliases(t) {
  if (!t.label) return '';
  const spaced = t.label.replace(/[^0-9a-z]+/gi, ' ');
  // Punctuation dropped, spaces kept - so words never fuse across a gap.
  const joined = t.label.replace(/[^0-9a-z\s]+/gi, '');
  const tiers = [...t.label.matchAll(/\$(\d+)/g)].map((m) => `tier ${m[1]}`).join(' ');
  return `${spaced} ${joined} ${tiers}`;
}

// The string a filter query is matched against. This IS the search
// vocabulary, so anything a person might reasonably type has to be in here -
// which is why the tier id, the CDNAME block and the curated label are all
// folded in even though only some of them are displayed on the cell.
//
// The whole thing is parenthesised before it is folded, and that is the point
// rather than a style choice: `.toLowerCase()` binds to the operand it is
// written on, so `a + b.toLowerCase()` folds only `b`. Written that way this
// function returned a haystack whose label half kept its capitals while the
// query arrived lowercased, and every disc-cased word became unsearchable -
// typing `ra-seru` matched nothing while "Ra-Seru Ozma $1" sat in the grid.
function texHaystack(t) {
  return (`${t.tier} ${texDesc(t)} ${t.width}x${t.height} ${t.bpp}bpp ${t.label || ''} ` +
    `${t.block || ''} ${t.replaceable ? 'replaceable' : 'read-only'} ${texAliases(t)}`)
    .toLowerCase();
}

// Stable identity of one queued edit. One edit per texture: re-adding one
// replaces the earlier edit rather than stacking a second write on it.
function texKey(q) {
  return `${q.tier}/${q.entry}/${q.section}/${q.offset}`;
}

// Wire the texture-replacement panel. `wasm()` resolves the module,
// `discBytes()` the current disc file's bytes. Returns { specs() } - the
// queued replacement specs for `apply_texture_replacements` ([] when idle).
function setupTextureReplacer(wasm, discBytes) {
  const scanBtn = $('rom-tex-scan');
  if (!scanBtn) return { specs() { return []; } };
  const scanNote = $('rom-tex-scan-note');
  const browser = $('rom-tex-browser');
  const filterInput = $('rom-tex-filter');
  const presetsEl = $('rom-tex-presets');
  const countEl = $('rom-tex-count');
  const grid = $('rom-tex-grid');
  const moreBtn = $('rom-tex-more');
  const editor = $('rom-tex-editor');
  const targetDesc = $('rom-tex-target-desc');
  const factsEl = $('rom-tex-facts');
  const exportBtn = $('rom-tex-export');
  const pngInput = $('rom-tex-png');
  const quantizeChk = $('rom-tex-quantize');
  const origCanvas = $('rom-tex-orig');
  const newCanvas = $('rom-tex-new');
  const verdict = $('rom-tex-verdict');
  const addBtn = $('rom-tex-add');
  const cancelBtn = $('rom-tex-cancel');
  const queueEl = $('rom-tex-queue');
  const packName = $('rom-tex-pack-name');
  const packAuthor = $('rom-tex-pack-author');
  const packNote = $('rom-tex-pack-note');
  const packExportBtn = $('rom-tex-pack-export');
  const packImportInput = $('rom-tex-pack-import');
  const packForceChk = $('rom-tex-pack-force');
  const packStatus = $('rom-tex-pack-status');
  const packReport = $('rom-tex-pack-report');
  const palPanel = $('rom-tex-palettes');
  const viewSel = $('rom-tex-view');
  const exportCompositeBtn = $('rom-tex-export-composite');
  const exportStripBtn = $('rom-tex-export-strip');
  const exportIndexedBtn = $('rom-tex-export-indexed');
  const regionsBox = $('rom-tex-regions');
  const regionsNote = $('rom-tex-regions-note');
  const regionsList = $('rom-tex-regions-list');

  const PAGE = 60;
  // The TIM families: the ones with real palettes the multi-palette tools
  // (views, composite / strip / indexed downloads) apply to.
  const isTimTier = (tier) => tier === 'raw' || tier === 'lzs';
  let rows = null; // scan result rows
  let tiers = []; // family descriptors from the registry
  let shown = 0;
  let sel = null; // { row, origImg, pngBytes }
  const queue = []; // { desc, tier, entry, section, offset, png, quantize, ... }

  const setNote = (msg, kind) => {
    scanNote.textContent = msg;
    scanNote.className = 'rom-hint' + (kind === 'err' ? ' rom-status-err' : '');
  };
  const setVerdict = (msg, kind) => {
    verdict.textContent = msg;
    verdict.className = 'rom-status' + (kind ? ' rom-status-' + kind : '');
  };
  const setPackStatus = (msg, kind) => {
    if (!packStatus) return;
    packStatus.textContent = msg;
    packStatus.className = 'rom-status' + (kind ? ' rom-status-' + kind : '');
  };
  const tierOf = (id) => tiers.find((t) => t.id === id) || null;

  function matches() {
    const q = (filterInput.value || '').trim().toLowerCase();
    if (!q) return rows;
    const toks = q.split(/\s+/);
    return rows.filter((t) => {
      const hay = texHaystack(t);
      return toks.every((tok) => hay.includes(tok));
    });
  }

  // Filter presets, built from what the scan actually returned rather than
  // from a guessed vocabulary: one chip per texture family, then one per
  // curated label present in this disc's rows, largest first. Every chip is
  // just filter text, so a person can see what it did and edit it.
  function renderPresets() {
    if (!presetsEl) return;
    presetsEl.textContent = '';
    const chips = [{ text: 'Everything', q: '', n: rows.length }];
    tiers.forEach((t) => {
      if (t.count > 0) chips.push({ text: t.title, q: t.id, n: t.count, tip: t.about });
    });
    // Label chips group on the label's leading segment. A curated label is
    // one closed-vocabulary word and groups to itself; a label a family
    // *composes* per row is unique per row ("Noa - Ra-Seru Terra $8"), so
    // without grouping every such row would become its own chip and bury
    // the strip. The lead segment is the useful shortcut anyway - it is the
    // character. Capped as well, so a future family cannot flood the strip
    // however its labels are shaped; a label past the cap is still typeable.
    const byLabel = new Map();
    rows.forEach((r) => {
      if (!r.label) return;
      const key = r.label.split(' - ')[0];
      byLabel.set(key, (byLabel.get(key) || 0) + 1);
    });
    [...byLabel.entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, 24)
      .forEach(([label, n]) => chips.push({ text: label, q: label, n }));

    const current = (filterInput.value || '').trim().toLowerCase();
    chips.forEach((c) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'rom-tex-preset' + (current === c.q.toLowerCase() ? ' is-active' : '');
      if (c.tip) b.title = c.tip;
      b.appendChild(document.createTextNode(c.text));
      const n = document.createElement('span');
      n.className = 'rom-tex-preset-n';
      n.textContent = c.n;
      b.appendChild(n);
      b.addEventListener('click', () => {
        filterInput.value = c.q;
        renderGrid(true);
        renderPresets();
      });
      presetsEl.appendChild(b);
    });
  }

  function renderGrid(reset) {
    if (reset) {
      grid.textContent = '';
      shown = 0;
    }
    const m = matches();
    if (countEl) {
      countEl.textContent = m.length === rows.length
        ? `${rows.length} textures.`
        : `${m.length} of ${rows.length} textures match.`;
    }
    const upto = Math.min(m.length, shown + PAGE);
    for (; shown < upto; shown++) {
      const t = m[shown];
      const cell = document.createElement('button');
      cell.type = 'button';
      cell.className = 'rom-tex-cell' + (t.replaceable ? '' : ' is-readonly');
      if (t.thumb) {
        const c = document.createElement('canvas');
        drawRgba(c, t.thumb);
        cell.appendChild(c);
      }
      const label = document.createElement('span');
      label.className = 'rom-tex-label';
      label.textContent = t.label || `${t.width}×${t.height}`;
      const sub = document.createElement('span');
      sub.textContent = `${texDesc(t)} · ${t.width}×${t.height} ${t.bpp}bpp` +
        (t.replaceable ? '' : ' · view only');
      cell.appendChild(label);
      cell.appendChild(sub);
      cell.addEventListener('click', () => select(t, cell));
      grid.appendChild(cell);
    }
    moreBtn.hidden = shown >= m.length;
    moreBtn.textContent = `Show more (${m.length - shown} left)`;
  }

  scanBtn.addEventListener('click', async () => {
    scanBtn.disabled = true;
    try {
      setNote('Reading disc image ...');
      const mod = await wasm();
      const buf = await discBytes();
      setNote('Scanning every texture (decompresses the whole disc - takes a moment) ...');
      await new Promise((r) => setTimeout(r, 30));
      const r = mod.scan_textures(buf, 48);
      rows = r.textures;
      tiers = r.tiers || [];
      browser.hidden = false;
      const families = tiers.filter((t) => t.count > 0)
        .map((t) => `${t.count} ${t.title.toLowerCase()}`)
        .join(', ');
      setNote(`${rows.length} textures found (${families}). Click one to edit it.`);
      renderGrid(true);
      renderPresets();
      offerStoredQueue();
    } catch (e) {
      setNote('Error: ' + (e && e.message ? e.message : e), 'err');
    } finally {
      scanBtn.disabled = false;
    }
  });
  filterInput.addEventListener('input', () => { renderGrid(true); renderPresets(); });
  moreBtn.addEventListener('click', () => renderGrid(false));

  // The facts a person needs to decide "is this the one, and can I change
  // it". Derived values only - nothing here is a guess about what a texture
  // depicts beyond the curated label the catalogs already carry.
  function renderFacts(t) {
    if (!factsEl) return;
    factsEl.textContent = '';
    const fam = tierOf(t.tier);
    const add = (k, v) => {
      if (v === null || v === undefined || v === '') return;
      const dt = document.createElement('dt');
      dt.textContent = k;
      const dd = document.createElement('dd');
      dd.textContent = v;
      factsEl.appendChild(dt);
      factsEl.appendChild(dd);
    };
    add('Family', fam ? fam.title : t.tier);
    add('Where', texDesc(t));
    add('PROT entry', t.entry < 0
      ? 'none - the unindexed gap before entry 0'
      : `${t.entry}${t.block ? ` (${t.block})` : ''}`);
    add('Pixels', `${t.width} × ${t.height}, ${t.bpp} bpp`);
    // "no palette of its own" is not "no palette" on the battle-art family:
    // such a block is still 4bpp and samples one a sibling block installed
    // on the shared row, so calling it direct colour would be wrong.
    // A monster sheet has no single colouring: each polygon picks a palette
    // with its CBA column, so the grid shows every texel through the palette
    // that actually reads it. Saying "1 of N" here would be a claim the
    // bytes do not make.
    add('Palettes', t.cluts > 0
      ? `${t.cluts}` + (t.tier === 'battle-equip' && t.cluts > 1
        ? ' - shown and replaced through the first; the others recolour the same pixels'
        : t.tier === 'monster'
          ? ' populated - each part of the model reads its own, and the preview uses them all'
          : '')
      : t.tier === 'battle-equip'
        ? 'none of its own - it borrows one another block installs'
        : 'none (direct colour)');
    add('Size on disc', `${t.bytes} bytes`);
    if (t.vram) add('VRAM', `(${t.vram.x}, ${t.vram.y}), ${t.vram.w} × ${t.vram.h}`);
    if (t.clut_vram) add('Palette in VRAM', `(${t.clut_vram.x}, ${t.clut_vram.y})`);
    add('Fingerprint', t.fnv1a);
    if (!t.replaceable) {
      add('Replaceable', `no - ${fam ? fam.about : 'view and export only'}`);
    } else if (t.tier === 'lzs') {
      add('Replaceable', 'yes, if your edit re-compresses into the retail stream ' +
        '(the preview measures it exactly)');
    } else if (t.tier === 'battle-equip') {
      // Not the LZS tier's budget: this record's slot is pinned by the
      // descriptor chain, and retail leaves as little as two spare bytes in
      // one, so "it did not fit" is a normal answer here.
      add('Replaceable', 'yes, if your edit re-compresses into this record\'s own ' +
        'slot (some are within a few bytes of full - the preview measures it exactly)');
    } else if (t.tier === 'monster') {
      // The palettes are off-limits here, and that is not a shortcut: a
      // monster's CLUTs upload to VRAM verbatim, so the bit that marks a
      // colour semi-transparent is live state a PNG cannot carry.
      add('Replaceable', 'yes, if your edit re-compresses into this monster\'s own ' +
        'archive slot. The palettes are never rewritten - repaint with the colors already ' +
        'in the exported sheet, or tick "fold" to snap strays onto the nearest one');
    } else {
      add('Replaceable', `yes - written in place, same ${t.bytes} bytes`);
    }
  }

  async function select(t, cell) {
    grid.querySelectorAll('.rom-tex-cell').forEach((c) => c.classList.remove('is-active'));
    if (cell) cell.classList.add('is-active');
    sel = { row: t, origImg: null, pngBytes: null, view: undefined, info: null };
    editor.hidden = false;
    targetDesc.textContent =
      `${texDesc(t)} · ${t.width}×${t.height} pixels · ${t.bpp} bpp · ` +
      `${t.cluts} palette(s)` + (t.label ? ` · ${t.label}` : '');
    renderFacts(t);
    pngInput.value = '';
    newCanvas.width = newCanvas.height = 0;
    addBtn.disabled = true;
    // A view-only family still shows and exports its texture; only the write
    // half is withheld, and it says why rather than silently doing nothing.
    pngInput.disabled = !t.replaceable;
    quantizeChk.disabled = !t.replaceable;
    setVerdict('Loading the full-size original ...');
    editor.scrollIntoView({ block: 'nearest' });
    await loadPalettes(t);
    await refresh();
  }

  // Palette views: only a TIM with more than one palette gets the selector
  // and the extra downloads. The in-game view is offered (and chosen) only
  // when the Rust side knows which palette each region is drawn with.
  async function loadPalettes(t) {
    if (!palPanel) return;
    palPanel.hidden = true;
    if (regionsBox) regionsBox.hidden = true;
    if (!isTimTier(t.tier) || !(t.cluts > 1)) return;
    try {
      const mod = await wasm();
      const buf = await discBytes();
      const info = mod.texture_palette_info(buf, t.tier, t.entry, t.section, t.offset);
      if (!sel || sel.row !== t || !(info.count > 1)) return;
      sel.info = info;
      viewSel.textContent = '';
      if (info.has_map) {
        const o = document.createElement('option');
        o.value = '-1';
        o.textContent = "In-game colors (each region's own palette)";
        viewSel.appendChild(o);
      }
      for (let k = 0; k < info.count; k++) {
        const o = document.createElement('option');
        o.value = String(k);
        o.textContent = `Palette ${k}`;
        viewSel.appendChild(o);
      }
      sel.view = info.has_map ? -1 : 0;
      viewSel.value = String(sel.view);
      renderRegions(info);
      palPanel.hidden = false;
    } catch (e) {
      // The texture still edits through palette 0 without the panel.
      palPanel.hidden = true;
    }
  }

  function renderRegions(info) {
    if (!regionsBox) return;
    regionsList.textContent = '';
    if (!info.has_map) {
      regionsBox.hidden = true;
      return;
    }
    const bits = [`From ${info.source}.`];
    if (info.unclaimed) {
      bits.push(`${info.unclaimed} pixel(s) no sprite in that table uses are shown through palette 0.`);
    }
    (info.notes || []).forEach((n) => bits.push('Note: ' + n + '.'));
    regionsNote.textContent = bits.join(' ');
    info.regions.forEach((r) => {
      const row = document.createElement('div');
      row.className = 'rom-tex-region-row';
      const pal = r.palette < info.count
        ? `palette ${r.palette}`
        : `palette ${r.subpalette} of a sibling texture (read-only here)`;
      row.textContent = `${r.x},${r.y} ${r.w}×${r.h} - ${pal}` +
        ` (${r.part}, widget ${r.widgets.map((w) => '0x' + w.toString(16).toUpperCase()).join(' ')})`;
      // Hover outlines the region on the original.
      row.addEventListener('mouseenter', () => outline(r));
      row.addEventListener('mouseleave', () => outline(null));
      regionsList.appendChild(row);
    });
    regionsBox.hidden = false;
  }

  function outline(r) {
    if (!sel || !sel.origImg) return;
    drawRgba(origCanvas, sel.origImg);
    if (!r) return;
    const ctx = origCanvas.getContext('2d');
    ctx.strokeStyle = '#ff00ff';
    ctx.lineWidth = 1;
    ctx.strokeRect(r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1);
  }

  if (viewSel) {
    viewSel.addEventListener('change', async () => {
      if (!sel) return;
      sel.view = Number(viewSel.value);
      await refresh();
    });
  }

  // Multi-palette downloads: encoded by the Rust side so the file is exactly
  // what the upload recognises.
  async function exportAs(format) {
    if (!sel) return;
    const t = sel.row;
    try {
      const mod = await wasm();
      const buf = await discBytes();
      const view = sel.view === undefined ? -1 : sel.view;
      const r = mod.export_texture_as(buf, t.tier, t.entry, t.section, t.offset, format, view);
      const suffix = format === 'image'
        ? (view < 0 ? '-ingame' : `-pal${view}`)
        : format === 'indexed' ? `-indexed-pal${view < 0 ? 0 : view}` : `-${format}`;
      downloadBlob(new Blob([r.png], { type: 'image/png' }), texFileName(t, suffix));
      setVerdict(`Downloaded: ${r.description}. Edit it and choose it above - the page recognises it.`);
    } catch (e) {
      setVerdict('Error: ' + (e && e.message ? e.message : e), 'err');
    }
  }
  if (exportCompositeBtn) exportCompositeBtn.addEventListener('click', () => exportAs('composite'));
  if (exportStripBtn) exportStripBtn.addEventListener('click', () => exportAs('strip'));
  if (exportIndexedBtn) exportIndexedBtn.addEventListener('click', () => exportAs('indexed'));

  function texFileName(t, suffix) {
    return t.tier === 'save-icon'
      ? `legaia-save-icon-slot${t.section}.png`
      : `legaia-tex-${t.tier}-${t.entry < 0 ? 'gap' : 'e' + t.entry}` +
        `${t.section >= 0 ? '-s' + t.section : ''}-0x${t.offset.toString(16)}${suffix || ''}.png`;
  }

  // Validate + preview. With no PNG chosen the call still returns the
  // original's full-size decode (the PNG error is expected and ignored). A
  // view-only family never reaches the writer at all - it decodes straight
  // off the disc.
  async function refresh() {
    if (!sel) return;
    const t = sel.row;
    try {
      const mod = await wasm();
      const buf = await discBytes();
      if (!t.replaceable) {
        sel.origImg = mod.decode_texture(buf, t.tier, t.entry, t.section, t.offset);
        drawRgba(origCanvas, sel.origImg);
        const fam = tierOf(t.tier);
        setVerdict('View and export only. ' + (fam ? fam.about : ''), 'warn');
        addBtn.disabled = true;
        return;
      }
      const png = sel.pngBytes || new Uint8Array(0);
      const r = mod.preview_texture_replace(
        buf, t.tier, t.entry, t.section, t.offset, png, quantizeChk.checked,
        isTimTier(t.tier) ? sel.view : undefined);
      sel.origImg = r.original;
      drawRgba(origCanvas, r.original);
      if (!sel.pngBytes) {
        setVerdict('Download the original, edit it (keep the size!), then choose your PNG above.');
        return;
      }
      if (r.preview) drawRgba(newCanvas, r.preview);
      if (r.ok) {
        const bits = ['Valid - ready to add.'];
        if (r.import_kind) bits.push(`Recognised as: ${r.import_kind}.`);
        if (r.new_palette_entries) bits.push(`${r.new_palette_entries} new palette color(s).`);
        if (r.palette_entries_changed) {
          bits.push(`${r.palette_entries_changed} palette entr${r.palette_entries_changed === 1 ? 'y' : 'ies'} ` +
            'change in total; every other palette color stays as it is.');
        }
        if (r.quantized_pixels) bits.push(`${r.quantized_pixels} pixel(s) folded to a nearest color.`);
        // Monster pages only: the parts of the sheet no polygon samples are
        // dead bytes, so paint there is reported rather than written. Saying
        // nothing would look like the edit landed and did nothing.
        if (r.dead_texels_ignored) {
          bits.push(`${r.dead_texels_ignored} pixel(s) fall where nothing on the model reads the ` +
            'sheet - those are left as they are.');
        }
        if (r.fit) bits.push(`Re-compresses to ${r.fit.recompressed} of the ${r.fit.capacity} available bytes.`);
        setVerdict(bits.join(' '), 'ok');
        addBtn.disabled = false;
      } else {
        setVerdict(r.error, 'err');
        addBtn.disabled = true;
      }
    } catch (e) {
      setVerdict('Error: ' + (e && e.message ? e.message : e), 'err');
      addBtn.disabled = true;
    }
  }

  pngInput.addEventListener('change', async () => {
    const f = pngInput.files && pngInput.files[0];
    sel.pngBytes = f ? new Uint8Array(await f.arrayBuffer()) : null;
    setVerdict('Checking your image ...');
    await refresh();
  });
  quantizeChk.addEventListener('change', refresh);

  // Download the selected texture's full-size decode as an editable PNG. A
  // multi-palette TIM downloads through the chosen view (Rust-encoded, so the
  // file is exactly what the upload recognises).
  exportBtn.addEventListener('click', () => {
    if (!sel || !sel.origImg) return;
    const t = sel.row;
    if (isTimTier(t.tier) && sel.info && sel.info.count > 1) {
      exportAs('image');
      return;
    }
    const c = document.createElement('canvas');
    drawRgba(c, sel.origImg);
    const name = texFileName(t, '');
    c.toBlob((blob) => {
      if (!blob) return;
      downloadBlob(blob, name);
    }, 'image/png');
  });

  function downloadBlob(blob, name) {
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 4000);
  }

  function renderQueue() {
    queueEl.textContent = '';
    queue.forEach((q, i) => {
      const row = document.createElement('div');
      row.className = 'rom-tex-queue-row';
      const label = document.createElement('span');
      label.textContent = `will replace: ${q.desc}${q.quantize ? ' (quantized)' : ''}`;
      const rm = document.createElement('button');
      rm.type = 'button';
      rm.className = 'art-remove';
      rm.textContent = '✕ Remove';
      rm.addEventListener('click', () => {
        queue.splice(i, 1);
        renderQueue();
      });
      row.appendChild(label);
      row.appendChild(rm);
      queueEl.appendChild(row);
    });
    storeQueue();
  }

  addBtn.addEventListener('click', () => {
    if (!sel || !sel.pngBytes) return;
    const t = sel.row;
    enqueue({
      desc: `${texDesc(t)} (${t.width}×${t.height}${t.label ? ', ' + t.label : ''})`,
      tier: t.tier, entry: t.entry, section: t.section, offset: t.offset,
      width: t.width, height: t.height, bpp: t.bpp, label: t.label || '',
      fnv1a: t.fnv1a,
      png: sel.pngBytes, quantize: quantizeChk.checked,
    });
    editor.hidden = true;
    sel = null;
  });
  cancelBtn.addEventListener('click', () => {
    editor.hidden = true;
    sel = null;
  });

  function enqueue(spec) {
    const existing = queue.findIndex((q) => texKey(q) === texKey(spec));
    if (existing >= 0) queue[existing] = spec; else queue.push(spec);
    renderQueue();
  }

  // --- Change packs ---------------------------------------------------------

  // The pack is also the persistence format: one serializer, one parser, one
  // set of coordinates. Anything that can be shared can be restored, and a
  // restore runs the same verification a stranger's pack does.
  function packJson() {
    return wasm().then((mod) => mod.export_texture_pack(
      queue.map((q) => ({
        tier: q.tier, entry: q.entry, section: q.section, offset: q.offset,
        png: q.png, quantize: q.quantize,
        fnv1a: q.fnv1a, width: q.width, height: q.height, bpp: q.bpp, label: q.label,
      })),
      (packName && packName.value) || '',
      (packAuthor && packAuthor.value) || '',
      (packNote && packNote.value) || '',
    ));
  }

  async function storeQueue() {
    try {
      if (!window.localStorage) return;
      if (!queue.length) {
        localStorage.removeItem(TEX_QUEUE_KEY);
        return;
      }
      localStorage.setItem(TEX_QUEUE_KEY, await packJson());
    } catch (e) {
      // A full or disabled localStorage must never break the editor - the
      // queue in memory is the real one.
    }
  }

  // Offer, never auto-apply: a stored edit was authored against whatever disc
  // was loaded last time, so it goes through the same verification an
  // imported pack does.
  function offerStoredQueue() {
    let stored = null;
    try {
      stored = window.localStorage && localStorage.getItem(TEX_QUEUE_KEY);
    } catch (e) { stored = null; }
    if (!stored || queue.length) return;
    setPackStatus('You have saved texture edits in this browser.');
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'rom-button rom-button-ghost';
    b.textContent = 'Restore my saved edits';
    b.addEventListener('click', () => { b.remove(); importPackText(stored); });
    packReport.textContent = '';
    packReport.appendChild(b);
  }

  if (packExportBtn) {
    packExportBtn.addEventListener('click', async () => {
      if (!queue.length) {
        setPackStatus('Queue up at least one texture edit first.', 'err');
        return;
      }
      try {
        const json = await packJson();
        const slug = ((packName && packName.value) || 'legaia-textures')
          .toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'legaia-textures';
        downloadBlob(new Blob([json], { type: 'application/json' }), `${slug}.pack.json`);
        setPackStatus(`Exported ${queue.length} texture edit(s). The pack holds your images and a fingerprint of each original - no game pixels.`, 'ok');
      } catch (e) {
        setPackStatus('Error: ' + (e && e.message ? e.message : e), 'err');
      }
    });
  }

  if (packImportInput) {
    packImportInput.addEventListener('change', async () => {
      const f = packImportInput.files && packImportInput.files[0];
      packImportInput.value = '';
      if (!f) return;
      importPackText(await f.text());
    });
  }

  async function importPackText(json) {
    packReport.textContent = '';
    try {
      setPackStatus('Checking the pack against your disc ...');
      const mod = await wasm();
      const buf = await discBytes();
      const r = mod.import_texture_pack(buf, json, packForceChk ? packForceChk.checked : false);
      if (packName && r.name) packName.value = r.name;
      if (packAuthor && r.author) packAuthor.value = r.author;
      if (packNote && r.note) packNote.value = r.note;
      let added = 0;
      r.entries.forEach((e) => {
        const row = document.createElement('div');
        row.className = 'rom-tex-pack-row ' + (e.usable ? 'is-ok' : 'is-bad');
        const where = document.createElement('code');
        where.textContent = texDesc(e);
        row.appendChild(where);
        row.appendChild(document.createTextNode(
          ` ${e.width}×${e.height}${e.label ? ', ' + e.label : ''} - ${e.detail}`));
        packReport.appendChild(row);
        if (!e.usable) return;
        enqueue({
          desc: `${texDesc(e)} (${e.width}×${e.height}${e.label ? ', ' + e.label : ''})`,
          tier: e.tier, entry: e.entry, section: e.section, offset: e.offset,
          width: e.width, height: e.height, bpp: e.bpp, label: e.label,
          fnv1a: e.fnv1a, png: e.png, quantize: e.quantize,
        });
        added++;
      });
      const skipped = r.entries.length - added;
      setPackStatus(
        `${added} of ${r.entries.length} texture(s) queued` +
        (skipped ? `; ${skipped} did not match this disc (see below).` : '.'),
        skipped ? 'warn' : 'ok');
    } catch (e) {
      setPackStatus('Error: ' + (e && e.message ? e.message : e), 'err');
    }
  }

  return {
    specs() {
      return queue.map((q) => ({
        tier: q.tier,
        entry: q.entry, section: q.section, offset: q.offset,
        png: q.png, quantize: q.quantize,
      }));
    },
  };
}

window.LegaiaRomPatcher = Object.assign(window.LegaiaRomPatcher || {}, {
  setupTextureReplacer,
});
