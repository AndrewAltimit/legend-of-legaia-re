/* play-pack-library.js - the "installed packs" list: expansion packs kept in
 * this browser, one of them active.
 *
 * Storage (a per-viewer convenience; every access is wrapped and the page
 * works without it): IndexedDB database `legaia-expansion`, store `packs` -
 * the same database and store as the workbench's hand-off slot (key `play`),
 * so neither side has to bump the schema version. Library keys:
 *
 *   `lib:<id>`  { id, name, version, authors, description, requires,
 *                 conflicts, bytes, cover, installedAt }   (`bytes` the
 *                 .lxpack, `cover` PNG bytes or null; `requires` /
 *                 `conflicts` the manifest's entries, `id` or `id@range`)
 *   `active`    the active pack's id, loaded when the play page has a disc
 *
 * The page never trusts the stored copy: every load goes back through the
 * engine (`load_expansion`), which re-runs the import gates and the
 * validator. Metadata for a new install comes from
 * `LegaiaRuntime.expansion_bundle_info` / `expansion_bundle_cover`, which
 * read the bundle through the same gates without activating it.
 *
 * Each row runs the format's dependency check against the rest of the
 * library (`LegaiaRuntime.expansion_library_check`, over
 * `manifest::check_dependencies`): an unmet `requires` (missing, or the
 * wrong version) and a `conflicts` entry naming an installed pack - the
 * active one called out - show under the pack. The engine plays one pack at
 * a time (docs/formats/expansion-pack.md#one-active-pack), so a met
 * requirement is listed but not loaded beside it.
 *
 * Classic script: `window.LegaiaPackLibrary` = { list, get, put, remove,
 * active, setActive, recordFor, attach }. The workbench calls `put` /
 * `recordFor` for "Add to library"; the play page calls `attach(api)` once:
 *   runtime()           the live LegaiaRuntime (or null)
 *   load(bytes, from)   load + activate a pack; the engine's info JSON
 *   status(text, bad)   the page's expansion status line
 *   mount               the element the list renders into
 */
(function () {
  'use strict';

  const DB = 'legaia-expansion';
  const STORE = 'packs';
  const PREFIX = 'lib:';
  const ACTIVE = 'active';

  function tx(mode, fn) {
    return new Promise((resolve) => {
      let req;
      try { req = indexedDB.open(DB, 1); } catch (e) { resolve(null); return; }
      req.onupgradeneeded = () => {
        try { req.result.createObjectStore(STORE); } catch (e) { /* exists */ }
      };
      req.onerror = () => resolve(null);
      req.onsuccess = () => {
        try {
          const t = req.result.transaction(STORE, mode);
          const r = fn(t.objectStore(STORE));
          let out = null;
          if (r) r.onsuccess = () => { out = r.result; };
          t.oncomplete = () => resolve(out);
          t.onerror = () => resolve(null);
          t.onabort = () => resolve(null);
        } catch (e) {
          resolve(null);
        }
      };
    });
  }

  /* Every installed pack, newest install first. */
  async function list() {
    const keys = (await tx('readonly', (s) => s.getAllKeys())) || [];
    const out = [];
    for (const k of keys) {
      if (typeof k !== 'string' || !k.startsWith(PREFIX)) continue;
      const rec = await tx('readonly', (s) => s.get(k));
      if (rec && rec.id) out.push(rec);
    }
    out.sort((a, b) => (b.installedAt || 0) - (a.installedAt || 0));
    return out;
  }

  const get = (id) => tx('readonly', (s) => s.get(PREFIX + id));
  const put = (rec) => tx('readwrite', (s) => s.put(rec, PREFIX + rec.id));
  const active = () => tx('readonly', (s) => s.get(ACTIVE));
  const setActive = (id) => tx('readwrite', (s) => (id ? s.put(id, ACTIVE) : s.delete(ACTIVE)));
  async function remove(id) {
    await tx('readwrite', (s) => s.delete(PREFIX + id));
    if ((await active()) === id) await setActive(null);
  }

  /* A library record from `.lxpack` bytes and the engine's reading of them
   * (`expansion_bundle_info`'s summary). */
  function recordFor(bytes, summary, cover) {
    return {
      id: summary.id,
      name: summary.name || summary.id,
      version: summary.version || '',
      authors: summary.authors || [],
      description: summary.description || '',
      requires: Array.isArray(summary.requires) ? summary.requires.slice() : [],
      conflicts: Array.isArray(summary.conflicts) ? summary.conflicts.slice() : [],
      bytes,
      cover: cover && cover.length ? cover : null,
      installedAt: Date.now(),
    };
  }

  // --- The play page's list ------------------------------------------------------

  function el(tag, attrs, ...kids) {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) {
      if (k === 'onclick') e.addEventListener('click', v);
      else if (k === 'class') e.className = v;
      else e.setAttribute(k, v);
    }
    for (const k of kids) if (k != null) e.append(k);
    return e;
  }

  function attach(api) {
    const mount = api.mount;
    let urls = [];

    /* Read `bytes` through the engine's import gates: the summary, or the
     * refusal worded for the player. */
    function inspect(bytes) {
      const rt = api.runtime();
      if (!rt || typeof rt.expansion_bundle_info !== 'function') {
        return { ok: false, error: { kind: 'no_engine', message: 'load the disc first' } };
      }
      try { return JSON.parse(rt.expansion_bundle_info(bytes)); } catch (e) {
        return { ok: false, error: { kind: 'corrupt', message: String(e && e.message ? e.message : e) } };
      }
    }

    function cover(bytes) {
      const rt = api.runtime();
      try { return rt && rt.expansion_bundle_cover ? rt.expansion_bundle_cover(bytes) : null; } catch (e) { return null; }
    }

    /* A record installed before the library kept `requires` / `conflicts`:
     * read them off its bytes once (and store them). */
    async function backfill(p) {
      if (Array.isArray(p.requires) && Array.isArray(p.conflicts)) return p;
      const info = p.bytes ? inspect(p.bytes instanceof Uint8Array ? p.bytes : new Uint8Array(p.bytes)) : null;
      if (!info || !info.ok) return p;
      p.requires = info.summary.requires || [];
      p.conflicts = info.summary.conflicts || [];
      await put(p);
      return p;
    }

    /* The dependency report per pack id, `{}` before the engine is up. */
    function check(packs, on) {
      const rt = api.runtime();
      if (!rt || typeof rt.expansion_library_check !== 'function') return {};
      const lib = {
        active: on || null,
        packs: packs.map((p) => ({ id: p.id, version: p.version || '', requires: p.requires || [], conflicts: p.conflicts || [] })),
      };
      try { return JSON.parse(rt.expansion_library_check(JSON.stringify(lib))); } catch (e) { return {}; }
    }

    /* One line per problem, worded for a player. */
    function depLines(rep, byId) {
      const out = [];
      if (!rep) return out;
      for (const pr of rep.problems || []) {
        const id = String(pr.dep).split('@')[0];
        const name = byId[id] ? byId[id].name : id;
        if (pr.kind === 'missing') {
          out.push({ bad: true, text: `Needs ${pr.dep}: not installed in this browser.` });
        } else if (pr.kind === 'wrong_version') {
          out.push({ bad: true, text: `Needs ${pr.dep}: ${name} ${pr.installed} is installed.` });
        } else if (pr.kind === 'conflict') {
          out.push({ bad: !!pr.active, warn: !pr.active, text: pr.active
            ? `Conflicts with ${name} ${pr.installed}, the active pack.`
            : `Conflicts with ${name} ${pr.installed} (installed).` });
        }
      }
      for (const u of rep.uses || []) {
        const id = String(u.dep).split('@')[0];
        const name = byId[id] ? byId[id].name : id;
        out.push({ text: `Builds on ${name} ${u.installed}; one pack plays at a time, so ${name}'s content is not loaded with it.` });
      }
      return out;
    }

    let lastReport = {};
    async function render() {
      if (!mount) return;
      for (const u of urls) URL.revokeObjectURL(u);
      urls = [];
      const packs = await list();
      const on = await active();
      for (const p of packs) await backfill(p);
      const byId = {};
      for (const p of packs) byId[p.id] = p;
      lastReport = check(packs, on);
      mount.innerHTML = '';
      if (!packs.length) {
        mount.append(el('div', { class: 'play-lib-empty' }, 'No packs installed in this browser yet.'));
        return;
      }
      for (const p of packs) {
        let img;
        if (p.cover && p.cover.byteLength) {
          const url = URL.createObjectURL(new Blob([p.cover], { type: 'image/png' }));
          urls.push(url);
          img = el('img', { class: 'play-lib-cover', src: url, alt: '' });
        } else {
          img = el('div', { class: 'play-lib-cover is-blank', 'aria-hidden': 'true' }, (p.name || '?').slice(0, 1));
        }
        const by = p.authors && p.authors.length ? ` by ${p.authors.join(', ')}` : '';
        const row = el('div', { class: 'play-lib-row' + (p.id === on ? ' is-active' : ''), 'data-pack': p.id },
          img,
          el('div', { class: 'play-lib-meta' },
            el('div', { class: 'play-lib-name' }, p.name, ' ', el('span', { class: 'play-lib-ver' }, p.version)),
            el('div', { class: 'play-lib-by' }, (by ? by.slice(4) : p.id) + (p.id === on ? ' - active' : ''))),
          el('div', { class: 'play-lib-actions' },
            el('button', { type: 'button', title: `Make ${p.name} the active pack and play with it`, onclick: () => play(p.id) }, p.id === on ? 'Reload' : 'Play'),
            el('button', { type: 'button', title: `Remove ${p.name} from this browser (your saves keep its progress)`, onclick: () => drop(p.id, p.name) }, 'Remove')));
        const lines = depLines(lastReport[p.id], byId);
        if (lines.length) {
          row.append(el('div', { class: 'play-lib-deps' }, ...lines.map((l) =>
            el('div', { class: 'play-lib-dep' + (l.bad ? ' is-bad' : l.warn ? ' is-warn' : '') }, l.text))));
        }
        mount.append(row);
      }
    }

    async function play(id) {
      const rec = await get(id);
      if (!rec || !rec.bytes) {
        api.status('That pack is no longer stored in this browser.', true);
        return render();
      }
      const bytes = rec.bytes instanceof Uint8Array ? rec.bytes : new Uint8Array(rec.bytes);
      const info = api.load(bytes, `${rec.name} ${rec.version}`);
      if (info && info.ok) {
        await setActive(id);
        warnDeps(id);
      }
      await render();
      return !!(info && info.ok);
    }

    /* After a load: say what the pack's dependency check found, on the
     * status line (the row lists it too). */
    function warnDeps(id) {
      const rep = lastReport[id];
      const bad = rep && (rep.problems || []).filter((p) => p.kind !== 'conflict' || p.active);
      if (bad && bad.length) {
        api.status(`Loaded, but ${bad.map((p) => p.text).join('; ')}. Content it takes from those packs is missing.`, true);
      }
    }

    /* Play installed pack `id` (the workbench's hand-off, `?lib=<id>`). */
    function loadId(id) {
      return play(id);
    }

    /* `End` on the play page: reload the active pack from its library copy
     * in place (`live.reload`, the workbench's hot reload), with the
     * notice the native window shows (`pack_reload_notice`'s wording). */
    async function reloadActive(live) {
      const notice = (t) => { if (typeof window.__playShowNotice === 'function') window.__playShowNotice(t); };
      const id = await active();
      const rec = id ? await get(id) : null;
      if (!rec || !rec.bytes || !live) { notice('Reload pack: no expansion pack is loaded'); return false; }
      const bytes = rec.bytes instanceof Uint8Array ? rec.bytes : new Uint8Array(rec.bytes);
      const r = live.reload(bytes, `${rec.name} ${rec.version}`);
      if (r && r.ok) {
        notice(`Reload pack: ${rec.id} ${rec.version} (${r.totalMs} ms)`);
      } else if (r && r.busy) {
        notice('Reload pack: waits until the fight or screen is over');
      } else {
        notice(`Reload pack refused: ${((r && r.errors) || []).slice(0, 1).join('')}`.slice(0, 56));
      }
      return !!(r && r.ok);
    }

    async function drop(id, name) {
      await remove(id);
      api.status(`Removed ${name} from this browser. A pack still active stays loaded until you unload it or leave the page.`);
      await render();
    }

    /* Install `bytes` (a file the player chose or dropped) and play it. A
     * pack already installed under the same id is replaced: the library
     * holds one version of each pack. */
    async function install(bytes, from) {
      const info = inspect(bytes);
      if (!info.ok) {
        api.status(`${from} was not installed: ${info.error.message}`, true);
        return false;
      }
      const s = info.summary;
      const prev = await get(s.id);
      const ok = await put(recordFor(bytes, s, cover(bytes)));
      if (ok === null && !window.indexedDB) {
        api.status('This browser keeps no storage here, so the pack is loaded but not installed.', true);
      }
      const loaded = api.load(bytes, from);
      if (loaded && loaded.ok) {
        await setActive(s.id);
        if (prev && prev.version && prev.version !== s.version) {
          api.status(`${s.name} updated ${prev.version} -> ${s.version} and is active. Saves made with ${prev.version} keep what still exists in ${s.version}.`);
        }
      }
      await render();
      return !!(loaded && loaded.ok);
    }

    /* The disc is up: play with the active pack, if one is set. */
    async function loadActive() {
      const id = await active();
      if (!id) return false;
      const rec = await get(id);
      if (!rec || !rec.bytes) { await setActive(null); return false; }
      const bytes = rec.bytes instanceof Uint8Array ? rec.bytes : new Uint8Array(rec.bytes);
      const info = api.load(bytes, `${rec.name} ${rec.version}`);
      await render();
      return !!(info && info.ok);
    }

    /* The player unloaded the pack: none is active next visit either. */
    async function deactivate() {
      await setActive(null);
      await render();
    }

    render();
    return { install, loadActive, loadId, reloadActive, deactivate, render };
  }

  window.LegaiaPackLibrary = { list, get, put, remove, active, setActive, recordFor, attach };
})();
