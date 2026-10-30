/* Expansion workbench, sharing: open a shared pack (the Open .lxpack button,
 * or a drop of an .lxpack or a pack folder anywhere on the page), and the
 * Share panel - the pack's identity card (cover, version, engine and format
 * gates, dependencies), a diff against an older .lxpack with a changelog
 * draft, the editable sources as a zip, and the play page's library.
 *
 * Every answer is the WASM session's (crates/web-expansion):
 *   import_bundle(bytes)       the import gates; a refusal names its kind
 *   share_status(pack)         compatibility, dependency parse, cover size
 *   diff_against(old, pack)    entry-by-entry diff, bump, changelog draft
 *   set_doc / doc_text         CHANGELOG.md
 *   sources_zip(pack)          the source tree under one <id>/ folder
 *
 * `attachShare(api)` is called once by expansion-workbench-app.js, with:
 *   S                the page state (S.wb the session, S.pack the pack JSON)
 *   changed(struct)  the editors' "something changed" hook
 *   setStatus(msg, kind, id), errText(e), downloadFile(blob, name)
 *   loaded(res, label, tree)   hand an import result to the page's loader
 *   loadTree(tree, label)      load a { path: bytes } tree
 *   needDisc()       true (after saying so) when no disc is loaded yet
 *   exportBundle()   the .lxpack bytes (throws while the pack has errors)
 *   play()           the page's Play in browser
 * and returns { importBundle(bytes, name), open(), refresh() }.
 */

import { h } from './expansion-workbench-forms.js';

/* What each import refusal means, for the person who dropped the file. */
const KIND_HINT = {
  not_a_bundle: 'This is not an .lxpack. Packs are shared as .lxpack files (or open a pack folder).',
  corrupt: 'The file is damaged or incomplete - download it again.',
  unsupported_version: 'It was made by a newer workbench. Update the site (or the engine) to open it.',
  too_large: 'It is over the bundle size limits.',
  bad_path: 'It holds a file path that could escape the pack folder, so it was refused.',
  newer_format: 'It needs a newer engine than this one.',
  older_format: 'It is written in a pack format this build no longer reads.',
  newer_engine: 'It needs a newer engine than this one.',
  unsafe: 'Packs are data only: no game files, no programs, only allowed asset types.',
  unparsable: 'Its pack.toml or a section does not parse.',
};

const STYLE = `
  .xw-share { margin: 0.4rem 0 0.6rem; }
  .xw-share[hidden] { display: none; }
  .xw-share-id { display: grid; grid-template-columns: 192px minmax(0, 1fr); gap: 0.8rem; align-items: start; }
  .xw-share-cover { width: 192px; aspect-ratio: 16 / 9; object-fit: cover; display: block; border-radius: 4px; background: var(--bg-code); border: 1px solid var(--border); }
  .xw-share-cover.is-blank { display: flex; align-items: center; justify-content: center; color: var(--text-dim); font-size: 0.85em; }
  .xw-share-facts { font-size: 0.88em; line-height: 1.55; }
  .xw-share-facts code { font-size: 0.92em; }
  .xw-share-ok { color: var(--ok, #3a9a5b); }
  .xw-share-bad { color: #d84b4b; }
  .xw-diff { font-size: 0.85em; margin-top: 0.4rem; }
  .xw-diff-file { margin: 0.4rem 0 0.2rem; font-family: var(--font-mono); color: var(--text-bright); }
  .xw-diff-row { display: grid; grid-template-columns: 1.2rem minmax(0, 1fr); gap: 0.3rem; padding: 0.1rem 0; }
  .xw-diff-row .xw-diff-mark { font-family: var(--font-mono); font-weight: 700; }
  .xw-diff-row.is-added .xw-diff-mark { color: var(--ok, #3a9a5b); }
  .xw-diff-row.is-removed .xw-diff-mark { color: #d84b4b; }
  .xw-diff-row.is-changed .xw-diff-mark { color: var(--accent); }
  .xw-diff-fields { margin: 0.1rem 0 0.2rem; padding-left: 0.8rem; font-family: var(--font-mono); font-size: 0.92em; color: var(--text-muted); overflow-wrap: anywhere; }
  .xw-share textarea { width: 100%; min-height: 10rem; font-family: var(--font-mono); font-size: 0.82em; box-sizing: border-box; }
  .xw-drop-veil { position: fixed; inset: 0; z-index: 50; display: flex; align-items: center; justify-content: center; background: rgba(0, 0, 0, 0.55); color: #fff; font-size: 1.2rem; pointer-events: none; }
  .xw-drop-veil[hidden] { display: none; }
  .xw-drop-veil span { padding: 1.2rem 1.8rem; border: 2px dashed currentColor; border-radius: 10px; }
  @media (max-width: 640px) {
    .xw-share-id { grid-template-columns: minmax(0, 1fr); }
    .xw-share-cover { width: 100%; }
  }
`;

export function attachShare(api) {
  const { S } = api;
  const style = document.createElement('style');
  style.textContent = STYLE;
  document.head.append(style);

  const panel = h('section', { class: 'xw-share xw-box', id: 'xw-share', hidden: true, 'aria-label': 'Share this pack' });
  const exportStatus = document.getElementById('xw-export-status');
  exportStatus.after(panel);

  const toggle = h('button', { type: 'button', class: 'wb-button wb-button-ghost', id: 'xw-share-btn', title: 'The pack\'s identity, a diff against an older version, its sources and the play library' }, 'Share');
  document.getElementById('xw-toml').after(toggle);
  toggle.addEventListener('click', () => {
    panel.hidden = !panel.hidden;
    if (!panel.hidden) render();
  });

  let diff = null;       // the last diff_against answer
  let logDraft = null;   // the changelog text once the author edits it
  let oldName = '';      // the older .lxpack's file name
  let coverUrl = null;

  // The panel re-renders on every edit; its status line survives that.
  let lastStatus = ['', ''];
  const status = (msg, kind) => {
    lastStatus = [msg, kind];
    if (document.getElementById('xw-share-status')) api.setStatus(msg, kind, 'xw-share-status');
  };
  const packJson = () => JSON.stringify(S.pack);

  // --- Import ----------------------------------------------------------------------

  /* Open a shared .lxpack through every import gate. */
  async function importBundle(bytes, name) {
    if (api.needDisc()) return false;
    let res;
    try { res = JSON.parse(S.wb.import_bundle(bytes)); } catch (e) {
      api.setStatus(`${name} was not opened: ${api.errText(e)}`, 'err');
      return false;
    }
    if (!res.ok) {
      const hint = KIND_HINT[res.error.kind] || '';
      api.setStatus(`${name} was not opened: ${res.error.message}. ${hint}`, 'err');
      return false;
    }
    diff = null;
    oldBytes = null;
    logDraft = null;
    await api.loaded(res, name, { bundle: bytes });
    if (!panel.hidden) render();
    return true;
  }

  /* A dropped folder, walked through the File and Directory Entries API. */
  function readEntry(entry, prefix, out) {
    return new Promise((resolve) => {
      if (entry.isFile) {
        entry.file(async (f) => {
          out[prefix + entry.name] = new Uint8Array(await f.arrayBuffer());
          resolve();
        }, () => resolve());
      } else if (entry.isDirectory) {
        if (/^(\.git|\.DS_Store|Thumbs\.db)$/.test(entry.name)) { resolve(); return; }
        const reader = entry.createReader();
        const all = [];
        const next = () => reader.readEntries(async (batch) => {
          if (!batch.length) {
            for (const e of all) await readEntry(e, `${prefix}${entry.name}/`, out);
            resolve();
          } else {
            all.push(...batch);
            next();
          }
        }, () => resolve());
        next();
      } else {
        resolve();
      }
    });
  }

  const veil = h('div', { class: 'xw-drop-veil', hidden: true }, h('span', {}, 'Drop an .lxpack or a pack folder to open it'));
  document.body.append(veil);
  let depth = 0;
  const carriesFiles = (e) => e.dataTransfer && [...(e.dataTransfer.types || [])].includes('Files');
  document.addEventListener('dragenter', (e) => {
    if (!carriesFiles(e)) return;
    depth += 1;
    veil.hidden = false;
  });
  document.addEventListener('dragleave', () => {
    depth = Math.max(0, depth - 1);
    if (!depth) veil.hidden = true;
  });
  document.addEventListener('dragover', (e) => {
    if (carriesFiles(e)) e.preventDefault();
  });
  document.addEventListener('drop', async (e) => {
    if (!carriesFiles(e)) return;
    // The disc picker's own drop handling (rom-cache.js) owns drops on it.
    if (e.target && e.target.closest && e.target.closest('input[type=file], .wb-file-drop')) return;
    e.preventDefault();
    depth = 0;
    veil.hidden = true;
    const items = [...(e.dataTransfer.items || [])];
    const entries = items.map((i) => (i.webkitGetAsEntry ? i.webkitGetAsEntry() : null)).filter(Boolean);
    if (entries.length === 1 && entries[0].isDirectory) {
      if (api.needDisc()) return;
      const tree = {};
      await readEntry(entries[0], '', tree);
      await api.loadTree(tree, `folder ${entries[0].name}`);
      return;
    }
    const f = e.dataTransfer.files && e.dataTransfer.files[0];
    if (!f) return;
    if (/\.(bin|iso|img|cue)$/i.test(f.name)) {
      api.setStatus('That looks like a disc image: choose it with the disc picker at the top.', 'err');
      return;
    }
    await importBundle(new Uint8Array(await f.arrayBuffer()), f.name);
  });

  // --- The panel ---------------------------------------------------------------------

  function coverImg(m) {
    if (coverUrl) { URL.revokeObjectURL(coverUrl); coverUrl = null; }
    const rel = m.cover && m.cover.startsWith('assets/') ? m.cover.slice(7) : null;
    const bytes = rel && S.wb.asset_bytes ? S.wb.asset_bytes(rel) : null;
    if (!bytes || !bytes.length) return h('div', { class: 'xw-share-cover is-blank' }, 'no cover');
    coverUrl = URL.createObjectURL(new Blob([bytes], { type: 'image/png' }));
    return h('img', { class: 'xw-share-cover', src: coverUrl, alt: `${m.name} cover` });
  }

  function setCover(file) {
    file.arrayBuffer().then((buf) => {
      const bytes = new Uint8Array(buf);
      if (!(bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e && bytes[3] === 0x47)) {
        status('A cover is a PNG image.', 'err');
        return;
      }
      S.wb.add_asset('cover.png', bytes);
      S.pack.manifest.cover = 'assets/cover.png';
      api.changed(false);
      render();
      status(`Cover set: assets/cover.png (${bytes.length} bytes).`, 'ok');
    });
  }

  function depList(label, rows) {
    if (!rows.length) return null;
    return h('div', {}, `${label}: `, rows.map((d, i) => [
      i ? ', ' : '',
      d.ok
        ? h('code', {}, d.range ? `${d.id} ${d.range}` : `${d.id} (any version)`)
        : h('code', { class: 'xw-share-bad', title: d.error }, `${d.text} (malformed)`),
    ]));
  }

  function identity() {
    const m = S.pack.manifest;
    let st = null;
    try { st = JSON.parse(S.wb.share_status(packJson())); } catch (e) { st = null; }
    const pick = h('input', { type: 'file', accept: 'image/png', hidden: true });
    pick.addEventListener('change', () => { if (pick.files[0]) setCover(pick.files[0]); pick.value = ''; });
    const facts = h('div', { class: 'xw-share-facts' },
      h('div', {}, h('strong', {}, m.name || m.id), ' ', h('code', {}, m.version), ' ', h('code', {}, m.id)),
      h('div', {}, m.authors && m.authors.length ? `by ${m.authors.join(', ')}` : h('span', { class: 'xw-share-bad' }, 'no authors listed')),
      st ? h('div', {},
        `Pack format ${st.schema} (this engine reads ${st.schema_min} to ${st.schema_max}); `,
        m.min_engine ? `needs engine ${m.min_engine}; ` : 'no minimum engine set; ',
        `this is engine ${st.engine}. `,
        st.compatible ? h('span', { class: 'xw-share-ok' }, 'Plays here.') : h('span', { class: 'xw-share-bad' }, st.why)) : null,
      st ? depList('Requires', st.requires) : null,
      st ? depList('Conflicts with', st.conflicts) : null,
      h('div', { class: 'xw-tools' },
        h('button', { type: 'button', class: 'wb-mini', onclick: () => pick.click() }, m.cover ? 'Replace cover' : 'Set cover image'),
        pick,
        h('span', { class: 'xw-hint' }, 'Version, minimum engine and dependencies are on the Manifest tab.')));
    return h('div', { class: 'xw-share-id' }, coverImg(m), facts);
  }

  function diffView() {
    const box = h('div', { class: 'xw-diff' });
    if (!diff) return box;
    const [a, r, c] = diff.counts;
    box.append(h('div', {},
      `Against ${oldName} (${diff.old.id} ${diff.old.version}): `,
      h('strong', {}, `${a} added, ${r} removed, ${c} changed`), '.',
      diff.same_pack ? '' : h('span', { class: 'xw-share-bad' }, ` That file is pack ${diff.old.id}, not ${S.pack.manifest.id}.`)));
    if (diff.suggested_version && diff.suggested_version !== S.pack.manifest.version) {
      box.append(h('div', { class: 'xw-tools' },
        `The changes call for a ${diff.bump === 'major' ? 'breaking (major)' : diff.bump} release: `,
        h('button', {
          type: 'button', class: 'wb-mini', onclick: () => {
            S.pack.manifest.version = diff.suggested_version;
            api.changed(false);
            runDiff(null);
          },
        }, `Set version to ${diff.suggested_version}`)));
    }
    for (const f of diff.diff.files) {
      box.append(h('div', { class: 'xw-diff-file' }, f.path));
      for (const e of f.entries) {
        const mark = { added: '+', removed: '-', changed: '~' }[e.change];
        const row = h('div', { class: `xw-diff-row is-${e.change}` },
          h('span', { class: 'xw-diff-mark' }, mark),
          h('span', {}, `${e.collection} `, h('code', {}, e.id)));
        box.append(row);
        if (e.change === 'changed' && e.fields && e.fields.length) {
          box.append(h('div', { class: 'xw-diff-fields' },
            e.fields.slice(0, 40).map((fc) => h('div', {}, `${fc.field || '(value)'}: ${fc.old ?? '(none)'} -> ${fc.new ?? '(none)'}`)),
            e.fields.length > 40 ? h('div', {}, `... and ${e.fields.length - 40} more`) : null));
        }
      }
    }
    const log = h('textarea', { 'aria-label': 'Changelog draft', spellcheck: 'true' });
    log.value = logDraft != null ? logDraft : diff.changelog;
    log.addEventListener('input', () => { logDraft = log.value; });
    box.append(h('h4', {}, 'Changelog draft'), log, h('div', { class: 'xw-tools' },
      h('button', {
        type: 'button', class: 'wb-mini', onclick: async (ev) => {
          try { await navigator.clipboard.writeText(log.value); ev.target.textContent = 'Copied'; } catch (e) { ev.target.textContent = 'Copy failed'; }
        },
      }, 'Copy'),
      h('button', {
        type: 'button', class: 'wb-mini', title: 'Put this section at the top of the pack\'s CHANGELOG.md (created if absent)', onclick: () => {
          const old = S.wb.doc_text('CHANGELOG.md');
          const head = old.startsWith('# ') ? old.slice(0, old.indexOf('\n') + 1) : '# Changelog\n';
          const rest = old.startsWith('# ') ? old.slice(head.length) : old;
          const text = `${head}\n${log.value.trim()}\n${rest ? `\n${rest.replace(/^\n+/, '')}` : ''}`;
          try {
            S.wb.set_doc('CHANGELOG.md', text);
            api.changed(false);
            status('Added to CHANGELOG.md; it ships in the next .lxpack.', 'ok');
          } catch (e) {
            status(api.errText(e), 'err');
          }
        },
      }, 'Add to CHANGELOG.md')));
    return box;
  }

  let oldBytes = null;
  function runDiff(bytes) {
    if (bytes) { oldBytes = bytes; logDraft = null; }
    if (!oldBytes) return;
    try {
      diff = JSON.parse(S.wb.diff_against(oldBytes, packJson()));
    } catch (e) {
      diff = null;
      status(`Cannot compare: ${api.errText(e)}`, 'err');
    }
    render();
  }

  async function addToLibrary() {
    const lib = window.LegaiaPackLibrary;
    if (!lib) { status('The pack library script did not load.', 'err'); return; }
    try {
      const bytes = await api.exportBundle();
      const m = S.pack.manifest;
      const rel = m.cover && m.cover.startsWith('assets/') ? m.cover.slice(7) : null;
      const cover = rel ? S.wb.asset_bytes(rel) : null;
      const ok = await lib.put(lib.recordFor(bytes, m, cover));
      if (ok === null && !window.indexedDB) throw new Error('this browser keeps no storage here');
      await lib.setActive(m.id);
      status(`${m.name} ${m.version} is in the play page's library and active: the play page loads it with your disc.`, 'ok');
    } catch (e) {
      status(`Not added: ${api.errText(e)}`, 'err');
    }
  }

  function render() {
    if (panel.hidden || !S.pack) return;
    panel.innerHTML = '';
    const older = h('input', { type: 'file', accept: '.lxpack,application/octet-stream', hidden: true });
    older.addEventListener('change', async () => {
      const f = older.files && older.files[0];
      older.value = '';
      if (!f) return;
      oldName = f.name;
      runDiff(new Uint8Array(await f.arrayBuffer()));
    });
    panel.append(
      h('div', { class: 'xw-box-head' }, 'Share', h('span', { class: 'xw-sp' }),
        h('button', { type: 'button', class: 'wb-mini', onclick: () => { panel.hidden = true; } }, 'Close')),
      identity(),
      h('div', { class: 'xw-tools', style: 'margin-top: 0.6rem' },
        h('button', { type: 'button', class: 'wb-mini', title: 'Store the .lxpack in this browser\'s play library and make it the active pack', onclick: addToLibrary }, 'Add to play library'),
        h('button', { type: 'button', class: 'wb-mini', title: 'Store the pack and open the play page with it (the Play in browser hand-off)', onclick: () => api.play() }, 'Play'),
        h('span', { class: 'xw-sep' }),
        h('button', {
          type: 'button', class: 'wb-mini', title: 'The pack as editable files, under one folder: unzip it and Open folder reads it back', onclick: () => {
            try {
              const zip = S.wb.sources_zip(packJson());
              const name = `${S.pack.manifest.id || 'pack'}-${S.pack.manifest.version || 'src'}.zip`;
              api.downloadFile(new Blob([zip], { type: 'application/zip' }), name);
              status(`Downloaded ${name}: the pack's sources, ${zip.length} bytes.`, 'ok');
            } catch (e) {
              status(`Not exported: ${api.errText(e)}`, 'err');
            }
          },
        }, 'Download sources (.zip)'),
        h('span', { class: 'xw-sep' }),
        h('button', { type: 'button', class: 'wb-mini', title: 'Compare this pack with an earlier .lxpack of it, entry by entry', onclick: () => older.click() }, diff ? 'Compare with another .lxpack' : 'Compare with an older .lxpack'),
        older),
      h('div', { id: 'xw-share-status', class: 'wb-status' }),
      diffView());
    if (lastStatus[0]) api.setStatus(lastStatus[0], lastStatus[1], 'xw-share-status');
  }

  return {
    importBundle,
    open: () => { panel.hidden = false; render(); },
    /* The pack changed: a diff on screen is recomputed. */
    refresh: () => { if (!panel.hidden) { if (oldBytes) runDiff(null); else render(); } },
  };
}
