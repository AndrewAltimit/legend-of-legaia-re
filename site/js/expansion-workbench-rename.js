/* Expansion workbench: rename an id with every reference to it.
 *
 * The model is the format crate's (`legaia_expansion_format::xref`, through
 * `ExpansionWorkbench::rename_plan` / `rename_id`): the same index Find
 * lists, so a rename touches exactly the places Find shows. The panel
 * previews every field the rename rewrites (file, field path, old and new
 * value) as the new id is typed; Apply replaces the pack in one step of the
 * page's undo.
 *
 * `openRename(host, o)`: `o.kind`, `o.id` the id to rename; `o.plan(kind,
 * old, new)` and `o.apply(kind, old, new)` the wasm calls (JSON results);
 * `o.jump({ path, field })`; `o.done(message)` after a rename.
 */

import { h } from './expansion-workbench-forms.js';

const KIND_LABEL = {
  monster: 'monster', formation: 'formation', boss: 'boss', encounter: 'encounter table', quest: 'quest',
  challenge: 'challenge', flag: 'pack flag', counter: 'pack counter', card: 'card', booster: 'booster',
  dealer: 'card dealer', item: 'pack item', item_set: 'item set', shop: 'shop', art: 'art', art_chain: 'art chain',
  spell: 'spell', map: 'map', npc: 'NPC', cutscene: 'cutscene', line: 'dialogue line', mode: 'mode',
  track: 'music track', sound: 'sound',
};
export const kindLabel = (k) => KIND_LABEL[k] || k;

const ROLE_LABEL = { defines: 'definition', sets: 'writes', reads: 'reads', references: 'uses' };

let styled = false;
function style() {
  if (styled) return;
  styled = true;
  document.head.append(h('style', {}, `
  .xw-rename { margin: 0.4rem 0; padding: 0.5rem 0.6rem; border: 1px solid var(--accent-line); border-radius: 6px; background: var(--accent-soft); font-size: 0.9em; }
  .xw-rename-row { display: flex; flex-wrap: wrap; gap: 0.4rem; align-items: center; }
  .xw-rename-row input { flex: 1 1 10rem; min-width: 0; font-family: var(--font-mono); }
  .xw-rename-list { max-height: 14rem; overflow: auto; margin-top: 0.35rem; }
  .xw-rename-ch { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 0 0.5rem; width: 100%; text-align: left; padding: 0.15rem 0.35rem; margin: 0 0 1px; font: inherit; font-size: 0.86em; color: var(--text); background: var(--bg-raised); border: none; border-left: 3px solid var(--accent-line); border-radius: 3px; cursor: pointer; overflow-wrap: anywhere; }
  .xw-rename-ch:hover { background: var(--bg-card-hi); }
  .xw-rename-ch .xw-rn-diff { grid-column: 1 / -1; font-family: var(--font-mono); color: var(--text-dim); }
  .xw-rename-ch del { color: var(--warn); text-decoration: line-through; }
  .xw-rename-ch ins { color: var(--ok); text-decoration: none; }
  .xw-rename-err { color: var(--warn); }
  `));
}

/** The rename panel for one id, appended to `host`; returns its element. */
export function openRename(host, o) {
  style();
  const input = h('input', { type: 'text', value: o.id, spellcheck: 'false', autocomplete: 'off', 'aria-label': `New ${kindLabel(o.kind)} id` });
  const apply = h('button', { type: 'button', class: 'wb-mini', disabled: true }, 'Rename');
  const cancel = h('button', { type: 'button', class: 'wb-mini' }, 'Cancel');
  const msg = h('div', { class: 'xw-dim', 'aria-live': 'polite' });
  const list = h('div', { class: 'xw-rename-list' });
  const box = h('div', { class: 'xw-rename', role: 'group', 'aria-label': `Rename ${kindLabel(o.kind)} ${o.id}` },
    h('div', { class: 'xw-rename-row' }, h('span', {}, `Rename ${kindLabel(o.kind)} `, h('code', {}, o.id), ' to'), input, apply, cancel),
    msg, list);
  let timer = null;
  let ready = false;
  const preview = () => {
    const next = input.value.trim();
    list.innerHTML = '';
    ready = false;
    apply.disabled = true;
    if (!next || next === o.id) { msg.className = 'xw-dim'; msg.textContent = 'Type the new id; every field that names this one is listed here first.'; return; }
    let res;
    try { res = o.plan(o.kind, o.id, next); } catch (e) { res = { ok: false, error: { message: String(e && e.message ? e.message : e) } }; }
    if (!res.ok) { msg.className = 'xw-rename-err'; msg.textContent = res.error.message; return; }
    const files = new Set(res.changes.map((c) => c.file));
    msg.className = 'xw-dim';
    msg.textContent = `${res.changes.length} field(s) in ${files.size} file(s) change, as one undo step:`;
    for (const c of res.changes) {
      list.append(h('button', { type: 'button', class: 'xw-rename-ch', onclick: () => o.jump({ path: c.file, field: c.field }) },
        h('span', {}, h('span', { class: 'xw-hit-file' }, c.file), ' ', h('code', {}, c.field || '(file name)')),
        h('span', { class: 'xw-dim' }, c.key ? 'key' : ROLE_LABEL[c.role] || c.role),
        h('span', { class: 'xw-rn-diff' }, h('del', {}, c.before), ' -> ', h('ins', {}, c.after))));
    }
    ready = true;
    apply.disabled = false;
  };
  const run = () => {
    if (!ready) return;
    const next = input.value.trim();
    let res;
    try { res = o.apply(o.kind, o.id, next); } catch (e) { res = { ok: false, error: { message: String(e && e.message ? e.message : e) } }; }
    if (!res.ok) { msg.className = 'xw-rename-err'; msg.textContent = res.error.message; return; }
    box.remove();
    res.newId = next;
    o.done(res, `Renamed ${kindLabel(o.kind)} ${o.id} to ${next}: ${res.changes.length} field(s).`);
  };
  input.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(preview, 150); });
  input.addEventListener('keydown', (ev) => {
    if (ev.key === 'Enter') { ev.preventDefault(); clearTimeout(timer); preview(); run(); }
    if (ev.key === 'Escape') { ev.preventDefault(); ev.stopPropagation(); box.remove(); }
  });
  apply.addEventListener('click', run);
  cancel.addEventListener('click', () => box.remove());
  host.append(box);
  preview();
  input.focus();
  input.select();
  return box;
}
