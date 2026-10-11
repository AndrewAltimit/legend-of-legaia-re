/* rom-patcher-table-editors.js - structured editors over the disc's own tables.
 *
 * The equipment editor (Arts-bar command costs + equip owners) and the
 * prices-and-names editors (fishing prize prices, location renames). Each
 * reads its rows from the user's disc through the WASM module, and serialises
 * to the same token strings the raw (advanced) inputs feed patch_rom.
 *
 * Module script, loaded after rom-patcher-page-controls.js and ahead of
 * rom-patcher-app.js; publishes on window.LegaiaRomPatcher. */

const { $ } = window.LegaiaRomPatcher;


// --- Equipment editor: Arts-bar command costs + equip owners over the disc's own tables --
// One row per equippable item read from the user's disc via
// `read_equipment_table`. Each of the four Arts-bar commands is priced by
// the equipment section that fills it in that character's player battle
// file: Left / Right by the weapon and the Ra-Seru arm (Noa's weapon is her
// Right), Down / Up by the footwear (two records in one section). Weapons
// show that cost per character (30 favored / 42 off-class / 54 far; the
// Astral Sword is Vahn's one 54), Ra-Seru arms and footwear theirs (30 in
// retail), and every section's default record - what an unlisted item or an
// empty slot uses - is a row of its own. Every item shows its equip-owner
// bits as three checkboxes. Serializes to the CLI's
// `--swing-cost CHAR:ITEM[:up]=COST` / `--equip-owner ITEM=OWNERS` token
// lists, which is exactly what patch_rom takes, merged with the raw
// (advanced) inputs.
function setupEquipmentEditor(wasm, fileInput, discBytes) {
  const statusEl = $('rom-equip-status');
  const rowsEl = $('rom-equip-rows');
  const slotEl = $('rom-equip-slot');
  if (!rowsEl) {
    return { clear() {}, collect() { return { costs: '', owners: '', error: '' }; } };
  }
  const CHARS = ['Vahn', 'Noa', 'Gala'];
  const hexOf = (id) => '0x' + Number(id).toString(16).toUpperCase().padStart(2, '0');
  const setNote = (msg) => { if (statusEl) statusEl.textContent = msg; };
  let items = [];
  // Edits live here, not in the DOM: a slot change re-renders only
  // the visible rows, so a DOM-only edit on a hidden row would be lost.
  // costs: "key:char" -> typed string ('' = keep), where key is the item id
  // (its +0x04 record), "<id>u" (a footwear item's Up record), or a default
  // record: dw (weapon), dr (Ra-Seru), df (footwear Down), dfu (footwear Up).
  // owners: id -> mask.
  const costEdits = new Map();
  const ownerEdits = new Map();
  let defaults = [{}, {}, {}];
  let hands = [null, null, null];
  const DEFAULT_KEYS = { dw: 'weapon', dr: 'raseru', df: 'down', dfu: 'up' };
  const ARROW = { Left: 'L', Right: 'R', Down: '↓', Up: '↑' };
  // The retail Arts-input layout in miniature: four hexagonal pennants
  // sitting apart at north / east / south / west around a small d-pad
  // cross. The priced command's pennant is dark orange and drawn on the
  // game's own scale (`cost - 6` wide), so a typed cost visibly widens or
  // narrows it; the other three stay at retail's 30.
  const SVG_NS = 'http://www.w3.org/2000/svg';
  // Pennant anchors: Up / Down centred over the cross; Left / Right hold
  // their inner edge next to it and grow outward, so a wide pennant never
  // runs into the cross or the number box.
  const PENNANT_POS = { Up: [22, 6], Down: [22, 26], Left: [9, 16], Right: [35, 16] };
  const hexPoints = (cx, cy, w, h, dir) => {
    let x0 = cx - w / 2, x1 = cx + w / 2;
    if (dir === 'Left') { x1 = 15; x0 = x1 - w; }
    if (dir === 'Right') { x0 = 29; x1 = x0 + w; }
    const s = h / 2;
    return [[x0, cy], [x0 + s, cy - s], [x1 - s, cy - s], [x1, cy], [x1 - s, cy + s], [x0 + s, cy + s]]
      .map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
  };
  // Retail 30 -> 12 wide; the width tracks `cost - 6` at half scale.
  const pennantWidth = (cost) => Math.max(7, Math.min(20, (Number(cost) - 6) / 2));
  const dirGlyph = (cmd, cost) => {
    const svg = document.createElementNS(SVG_NS, 'svg');
    svg.setAttribute('viewBox', '0 0 44 32');
    svg.setAttribute('class', 'rom-equip-dpad');
    svg.setAttribute('aria-hidden', 'true');
    svg.dataset.cmd = cmd;
    for (const d of ['Up', 'Left', 'Right', 'Down']) {
      const poly = document.createElementNS(SVG_NS, 'polygon');
      const [cx, cy] = PENNANT_POS[d];
      poly.setAttribute('points', hexPoints(cx, cy, d === cmd ? pennantWidth(cost) : 12, 7, d));
      poly.setAttribute('class', d === cmd ? 'is-priced' : '');
      svg.appendChild(poly);
    }
    const cross = document.createElementNS(SVG_NS, 'path');
    cross.setAttribute('d', 'M20.5 13h3v2.5H26v3h-2.5V21h-3v-2.5H18v-3h2.5z');
    cross.setAttribute('class', 'hub');
    svg.appendChild(cross);
    return svg;
  };
  const sizeGlyph = (svg, cost) => {
    const poly = svg && svg.querySelector('polygon.is-priced');
    if (!poly) return;
    const [cx, cy] = PENNANT_POS[svg.dataset.cmd] || [22, 16];
    poly.setAttribute('points', hexPoints(cx, cy, pennantWidth(cost), 7, svg.dataset.cmd));
  };
  const otherHand = (h) => (h === 'Left' ? 'Right' : h === 'Right' ? 'Left' : null);
  // Owner mask as edited (falls back to the disc's).
  const effMask = (it) => (ownerEdits.has(it.id) ? ownerEdits.get(it.id) : it.mask);
  const canEquip = (it, ci) => (effMask(it) & (1 << ci)) !== 0;
  const newlyEnabled = (it, ci) => canEquip(it, ci) && (it.mask & (1 << ci)) === 0;
  // A weapon another character's file carries: when `ci` is newly ticked
  // on, the patcher carries that model over into their file (the record
  // keeps the donor's price). Ra-Seru levels (ids <= 0x1A) never move.
  const donorOf = (it, ci) => {
    if (it.slot !== 'weapon' || it.ra_seru_arm || it.id <= 0x1A) return null;
    const costs = it.costs || [];
    const cj = CHARS.findIndex((_, j) => j !== ci && costs[j] != null);
    return cj < 0 ? null : cj;
  };

  // The disc's current value for an edit key on one character (null = no
  // such record in that character's file).
  const curOf = (key, ci) => {
    if (key in DEFAULT_KEYS) {
      const v = (defaults[ci] || {})[DEFAULT_KEYS[key]];
      return v == null ? null : v;
    }
    const up = key.endsWith('u');
    const it = items.find((x) => x.id === parseInt(key, 10));
    if (!it) return null;
    const v = (up ? it.up_costs : it.costs) || [];
    if (v[ci] == null && !up && newlyEnabled(it, ci)) {
      const d = donorOf(it, ci);
      return d == null ? null : it.costs[d];
    }
    return v[ci] == null ? null : v[ci];
  };
  const tokenOf = (key, c) => {
    if (key === 'dw') return `${c}:default`;
    if (key === 'dr') return `${c}:raseru`;
    if (key === 'df') return `${c}:feet`;
    if (key === 'dfu') return `${c}:feet:up`;
    return `${c}:${hexOf(parseInt(key, 10))}${key.endsWith('u') ? ':up' : ''}`;
  };
  // What a default record currently shows: the typed edit, else the disc value.
  const shownOf = (key, ci) => (costEdits.get(`${key}:${CHARS[ci]}`) || '').trim() || (curOf(key, ci) == null ? '?' : String(curOf(key, ci)));

  const costInput = (key, c, cur, tr, title) => {
    const inp = document.createElement('input');
    inp.type = 'number';
    inp.className = 'eq-cost';
    inp.min = '24'; inp.max = '255';
    inp.placeholder = String(cur);
    inp.dataset.char = c;
    inp.dataset.key = key;
    inp.dataset.cur = String(cur);
    inp.value = costEdits.get(`${key}:${c}`) || '';
    inp.title = title;
    inp.addEventListener('input', () => {
      costEdits.set(`${key}:${c}`, inp.value.trim());
      rowEdited(tr);
      if (key in DEFAULT_KEYS) syncFallthrough(key, c);
    });
    return inp;
  };
  // A command cell: the d-pad glyph with the priced command lit, then the
  // number box; typing resizes the lit pennant.
  const cmdCell = (td, cmd, key, c, cur, tr, what) => {
    const lab = document.createElement('span');
    lab.className = 'rom-equip-cmd';
    lab.title = `${cmd} command`;
    const shown = (costEdits.get(`${key}:${c}`) || '').trim() || cur;
    const glyph = dirGlyph(cmd, shown);
    lab.appendChild(glyph);
    td.appendChild(lab);
    const inp = costInput(key, c, cur, tr, `${c}’s ${cmd} command with ${what}: currently ${cur} AP (${cur - 6} px wide)`);
    inp.addEventListener('input', () => sizeGlyph(glyph, inp.value.trim() || cur));
    td.appendChild(inp);
  };
  // A fall-through cell: the default record's value that applies instead.
  const fallthroughTitle = (c, cmd, def, row) => `${c}’s battle file has no section for this item. If ${c} equips it, the ${row} record is used: default look, ${def} AP per ${cmd} press. Change that in the ${row} row at the top.`;
  const fallthroughSpan = (key, c, ci, cmd, row) => {
    const sp = document.createElement('span');
    sp.className = 'rom-equip-na rom-equip-fallthrough';
    sp.dataset.char = c;
    sp.dataset.def = key;
    sp.dataset.cmd = cmd;
    sp.dataset.row = row;
    const def = shownOf(key, ci);
    sp.title = fallthroughTitle(c, cmd, def, row);
    sp.appendChild(dirGlyph(cmd, def));
    const txt = document.createElement('span');
    txt.className = 'rom-equip-fallthrough-val';
    txt.textContent = `↳ ${def}`;
    sp.appendChild(txt);
    return sp;
  };
  const syncFallthrough = (key, c) => {
    const ci = CHARS.indexOf(c);
    const def = shownOf(key, ci);
    for (const el of rowsEl.querySelectorAll(`.rom-equip-fallthrough[data-char="${c}"][data-def="${key}"]`)) {
      const v = el.querySelector('.rom-equip-fallthrough-val');
      if (v) v.textContent = `↳ ${def}`;
      sizeGlyph(el.querySelector('svg'), def);
      el.title = fallthroughTitle(c, el.dataset.cmd, def, el.dataset.row);
    }
  };

  const rowEdited = (tr) => {
    const keys = (tr.dataset.keys || '').split(',').filter(Boolean);
    const id = Number(tr.dataset.id);
    const it = Number.isFinite(id) ? items.find((x) => x.id === id) : null;
    const edited = keys.some((k) => CHARS.some((c) => (costEdits.get(`${k}:${c}`) || '') !== ''))
      || (it != null && ownerEdits.has(id) && ownerEdits.get(id) !== it.mask);
    tr.classList.toggle('is-edited', edited);
  };

  let showSlot = false;
  // A section-default row: `cells(ci)` returns the per-character cell content.
  const defaultRow = (tbody, label, note, keys, cells) => {
    const tr = document.createElement('tr');
    tr.className = 'rom-equip-row rom-equip-default';
    tr.dataset.keys = keys.join(',');
    tr.innerHTML = `<td><span class="rom-edit-name" title="${escapeHtml(note)}">${label}</span></td><td class="n">—</td><td>—</td>`;
    CHARS.forEach((c, ci) => {
      const td = document.createElement('td');
      td.className = 'n';
      cells(td, c, ci, tr);
      tr.appendChild(td);
    });
    rowEdited(tr);
    tbody.appendChild(tr);
  };
  const naCell = (td, why) => { td.innerHTML = `<span class="rom-equip-na" title="${escapeHtml(why)}">—</span>`; };
  const cannotEquip = (c) => `${c} cannot equip this. Tick ${c[0]} under “Who can equip” to allow it.`;
  const kicksCell = (td, c, ci, tr, keyDown, keyUp, what, rowName, it) => {
    // Footwear prices two commands: Down (+0x04 record) and Up (+0x08).
    if (it && !canEquip(it, ci)) { naCell(td, cannotEquip(c)); return; }
    const wrap = document.createElement('span');
    wrap.className = 'rom-equip-kicks';
    for (const [cmd, key] of [['Down', keyDown], ['Up', keyUp]]) {
      const line = document.createElement('span');
      const cur = curOf(key, ci);
      if (cur == null) {
        if (rowName) {
          line.appendChild(fallthroughSpan(key === keyDown ? 'df' : 'dfu', c, ci, cmd, rowName));
          if (it && newlyEnabled(it, ci)) td.classList.add('is-new');
        } else line.innerHTML = '<span class="rom-equip-na">—</span>';
      } else {
        cmdCell(line, cmd, key, c, cur, tr, what);
      }
      wrap.appendChild(line);
    }
    td.appendChild(wrap);
  };

  function render() {
    rowsEl.textContent = '';
    if (!items.length) {
      const p = document.createElement('p');
      p.className = 'rom-edit-empty';
      p.textContent = 'Waiting for a disc image ...';
      rowsEl.appendChild(p);
      return;
    }
    const slot = slotEl ? slotEl.value : 'weapon';
    const table = document.createElement('table');
    table.className = 'rom-equip-table';
    const thead = document.createElement('thead');
    // A mixed view names each row's slot under the item name; a per-slot
    // view already says it in the dropdown. No slot column - the table has
    // to fit half the group grid.
    showSlot = slot === 'all';
    thead.innerHTML = `<tr><th>Item</th><th class="n" title="Attack bonus">ATK</th><th title="Who can equip it: Vahn, Noa, Gala">Owners</th>`
      + '<th class="n" title="Vahn: AP per press">Vahn</th><th class="n" title="Noa: AP per press">Noa</th><th class="n" title="Gala: AP per press">Gala</th></tr>';
    table.appendChild(thead);
    const tbody = document.createElement('tbody');
    let shown = 0;
    if (slot === 'weapon' || slot === 'all') {
      defaultRow(tbody, 'Default weapon',
        'Each character’s battle file has one default weapon record. It is what they swing with when the equipped weapon has no section in their file (a weapon you ticked on for them below) or when nothing is equipped. One value per character, shared by every such weapon.',
        ['dw'],
        (td, c, ci, tr) => {
          const cur = curOf('dw', ci);
          if (cur == null || !hands[ci]) naCell(td, 'Not found in this file');
          else cmdCell(td, hands[ci], 'dw', c, cur, tr, 'an unlisted weapon or bare hands');
        });
      defaultRow(tbody, 'Default Ra-Seru',
        'The Ra-Seru section’s default record: the other hand’s command when the equipped Ra-Seru level has no section in the file, or none is equipped.',
        ['dr'],
        (td, c, ci, tr) => {
          const cur = curOf('dr', ci);
          const cmd = otherHand(hands[ci]);
          if (cur == null || !cmd) naCell(td, 'Not found in this file');
          else cmdCell(td, cmd, 'dr', c, cur, tr, 'no listed Ra-Seru');
        });
    }
    if (slot === 'footwear' || slot === 'all') {
      defaultRow(tbody, 'Default footwear',
        'The footwear section’s default record prices both kicks when the equipped footwear has no section in the file (footwear you ticked on for them below) or none is equipped.',
        ['df', 'dfu'],
        (td, c, ci, tr) => kicksCell(td, c, ci, tr, 'df', 'dfu', 'unlisted footwear or bare feet', null));
    }
    // Named gear first, then the Ra-Seru levels (Meta / Terra / Ozma), and
    // the unnamed ids (`item 0xHH`) at the very bottom.
    const rank = (it) => (!it.name ? 2 : it.ra_seru_arm ? 1 : 0);
    const ordered = [...items].sort((a, b) => rank(a) - rank(b));
    for (const it of ordered) {
      if (slot !== 'all' && it.slot !== slot) continue;
      shown++;
      const tr = document.createElement('tr');
      tr.className = 'rom-equip-row';
      tr.dataset.id = String(it.id);
      tr.dataset.keys = it.slot === 'footwear' ? `${it.id},${it.id}u` : String(it.id);
      const tdName = document.createElement('td');
      // Name only - the id and the shared-row note live in the tooltip so
      // the column stays narrow enough for the table to fit without a
      // horizontal scrollbar.
      const shared = it.shares_row_with && it.shares_row_with.length
        ? ' Shares its stat row with ' + it.shares_row_with.map(hexOf).join(', ') + ' - an owner change moves them too.'
        : '';
      tdName.innerHTML = `<span class="rom-edit-name" title="${escapeHtml('Item ' + hexOf(it.id) + '.' + shared)}">${escapeHtml(it.name || 'item ' + hexOf(it.id))}</span>`;
      if (shared) {
        const s = document.createElement('span');
        s.className = 'rom-equip-shared';
        s.title = shared.trim();
        s.textContent = '*';
        tdName.appendChild(s);
      }
      tr.appendChild(tdName);
      // A Ra-Seru level already says so in its name; the mixed view labels
      // the slot under the name.
      if (showSlot) { const sl = document.createElement('span'); sl.className = 'rom-equip-slot'; sl.textContent = it.ra_seru_arm ? 'Ra-Seru arm' : it.slot; tdName.appendChild(sl); }
      const tdAtk = document.createElement('td'); tdAtk.className = 'n'; tdAtk.textContent = String(it.atk); tr.appendChild(tdAtk);
      const tdOwn = document.createElement('td');
      tdOwn.className = 'rom-equip-owners';
      const mask = ownerEdits.has(it.id) ? ownerEdits.get(it.id) : it.mask;
      CHARS.forEach((c, ci) => {
        const lab = document.createElement('label');
        const cb = document.createElement('input');
        cb.type = 'checkbox';
        cb.className = 'eq-owner';
        cb.dataset.bit = String(1 << ci);
        cb.dataset.cur = (it.mask & (1 << ci)) ? '1' : '0';
        cb.checked = (mask & (1 << ci)) !== 0;
        cb.addEventListener('change', () => {
          const cur = effMask(it);
          ownerEdits.set(it.id, cb.checked ? (cur | (1 << ci)) : (cur & ~(1 << ci)));
          // The cost cells follow the tick (dash / carried-over model /
          // default record), so the row is rebuilt from state.
          render();
        });
        lab.appendChild(cb);
        lab.appendChild(document.createTextNode(c));
        lab.title = `${c} can equip`;
        tdOwn.appendChild(lab);
      });
      tr.appendChild(tdOwn);
      CHARS.forEach((c, ci) => {
        const td = document.createElement('td');
        td.className = 'n';
        const cur = it.costs ? it.costs[ci] : null;
        const cmd = it.cmds ? it.cmds[ci] : null;
        if (it.slot === 'body' || it.slot === 'head') {
          naCell(td, canEquip(it, ci) ? 'Body and head gear price no command' : cannotEquip(c));
        } else if (it.slot === 'footwear') {
          kicksCell(td, c, ci, tr, String(it.id), `${it.id}u`, 'this footwear', 'Default footwear', it);
        } else if (!canEquip(it, ci)) {
          naCell(td, cannotEquip(c));
        } else if (cur == null || !cmd) {
          const donor = newlyEnabled(it, ci) ? donorOf(it, ci) : null;
          if (donor != null && hands[ci]) {
            // Newly ticked on, and another file has the model: the patcher
            // carries it over, at the donor's price unless typed over.
            const dcost = it.costs[donor];
            cmdCell(td, hands[ci], String(it.id), c, dcost, tr, `this weapon (model carried over from ${CHARS[donor]}’s file)`);
            td.classList.add('is-new');
            td.title = `${c} will hold ${CHARS[donor]}’s ${it.name || 'weapon'} model in battle; ${dcost} AP per swing (${CHARS[donor]}’s price) unless you type another.`;
          } else {
            const key = it.ra_seru_arm ? 'dr' : 'dw';
            const fcmd = it.ra_seru_arm ? otherHand(hands[ci]) : hands[ci];
            td.appendChild(fallthroughSpan(key, c, ci, fcmd || 'Left', it.ra_seru_arm ? 'Default Ra-Seru arm' : 'Default weapon'));
            if (newlyEnabled(it, ci)) td.classList.add('is-new');
          }
        } else {
          cmdCell(td, cmd, String(it.id), c, cur, tr, 'this ' + (it.ra_seru_arm ? 'Ra-Seru' : 'weapon'));
        }
        tr.appendChild(td);
      });
      rowEdited(tr);
      tbody.appendChild(tr);
    }
    table.appendChild(tbody);
    rowsEl.appendChild(table);
    if (!shown) {
      const p = document.createElement('p');
      p.className = 'rom-edit-empty';
      p.textContent = 'No items match.';
      rowsEl.appendChild(p);
    }
  }

  // Every edit key the disc has a record for, across all rows and defaults.
  const allKeys = () => {
    const keys = Object.keys(DEFAULT_KEYS);
    for (const it of items) {
      keys.push(String(it.id));
      if (it.slot === 'footwear') keys.push(`${it.id}u`);
    }
    return keys;
  };
  // Quick edits over every record the disc lists (not just the visible rows).
  const setCost = (pred, value) => {
    for (const key of allKeys()) {
      CHARS.forEach((c, ci) => {
        const cur = curOf(key, ci);
        if (cur == null || !pred(key, c, cur)) return;
        costEdits.set(`${key}:${c}`, cur === value ? '' : String(value));
      });
    }
    render();
  };
  const presets = {
    'rom-equip-preset-astral': () => setCost((key) => key === '186', 30),
    'rom-equip-preset-all30': () => setCost(() => true, 30),
    'rom-equip-preset-anyone': () => {
      const slot = slotEl ? slotEl.value : 'weapon';
      for (const it of items) if (slot === 'all' || it.slot === slot) ownerEdits.set(it.id, 7);
      render();
    },
    'rom-equip-preset-reset': () => clear(),
  };
  for (const [id, fn] of Object.entries(presets)) {
    const b = $(id);
    if (b) b.addEventListener('click', (e) => { e.preventDefault(); fn(); });
  }
  if (slotEl) slotEl.addEventListener('change', render);

  let loadedFor = null;
  async function load() {
    const file = fileInput.files && fileInput.files[0];
    if (!file) return;
    const key = `${file.name}/${file.size}/${file.lastModified}`;
    if (key === loadedFor) return;
    try {
      setNote('Reading equipment from your disc ...');
      const mod = await wasm();
      if (typeof mod.read_equipment_table !== 'function') {
        setNote('This patcher build cannot list equipment here - the raw lists under "Advanced" still work.');
        return;
      }
      const buf = await discBytes();
      const t = mod.read_equipment_table(buf);
      items = t.items || [];
      defaults = Array.isArray(t.defaults) ? t.defaults : [{}, {}, {}];
      hands = Array.isArray(t.weapon_hand) ? t.weapon_hand : [null, null, null];
      loadedFor = key;
      render();
      setNote('Read from your disc. Empty box = keep the disc value.');
    } catch (e) {
      setNote('Could not read the equipment table: ' + (e && e.message ? e.message : e));
    }
  }
  fileInput.addEventListener('change', () => { load(); });

  function clear() {
    costEdits.clear();
    ownerEdits.clear();
    render();
  }
  function collect() {
    const fail = (error) => ({ costs: '', owners: '', error });
    const costs = [];
    const owners = [];
    for (const [k, v] of costEdits) {
      const val = (v || '').trim();
      if (!val) continue;
      const i = k.lastIndexOf(':');
      const key = k.slice(0, i);
      const c = k.slice(i + 1);
      const ci = CHARS.indexOf(c);
      const n = parseInt(val, 10);
      if (!Number.isFinite(n) || n < 24 || n > 255) return fail(`Cost for ${c} (${tokenOf(key, c)}) must be 24..255 (below 24 the command label no longer fits its pennant).`);
      const cur = curOf(key, ci);
      if (cur == null || n === cur) continue;
      costs.push(`${tokenOf(key, c)}=${n}`);
    }
    for (const it of items) {
      if (ownerEdits.has(it.id) && ownerEdits.get(it.id) !== it.mask) {
        const m = ownerEdits.get(it.id);
        const letters = CHARS.filter((c, ci) => m & (1 << ci)).map((c) => c[0]).join('');
        owners.push(`${hexOf(it.id)}=${letters || 'none'}`);
      }
    }
    return { costs: costs.join(','), owners: owners.join(','), error: '' };
  }
  render();
  return { load, clear, collect };
}

const escapeHtml = window.escapeHtml;

// --- Prices & names: structured editors over the disc's own tables ----------
// Friendly rows over the same `fishing_prices` / `location_renames` strings
// the raw (advanced) inputs feed - the structured editors serialize to that
// exact syntax and the two sources are merged, so the patch_rom wire format is
// unchanged. Current values come from the user's own disc via
// `read_manual_edit_tables` (populated when a disc is chosen; placeholders
// before that). Capability bounds mirror the patcher's: a fishing prize can
// only be REPRICED (the 12 rows and their items are fixed on the disc, and a
// price is keyed by item id across both venues), and a location slot can only
// be RENAMED (16 fixed slots, ASCII, 31 chars).
function setupManualTables(wasm, fileInput, discBytes) {
  const statusEl = $('rom-tables-status');
  const fishRowsEl = $('rom-fish-rows');
  const locRowsEl = $('rom-loc-rows');
  if (!fishRowsEl || !locRowsEl) {
    return { clear() {}, collect() { return { fishing: '', locations: '', error: '' }; } };
  }

  // Overwritten from the disc read; the shared three-carrier cap is 23
  // (the world-map record's name field), which the WASM export reports.
  let maxNameLen = 23;

  const setNote = (msg) => { if (statusEl) statusEl.textContent = msg; };
  const showEmpty = (el, msg) => {
    el.textContent = '';
    const p = document.createElement('p');
    p.className = 'rom-edit-empty';
    p.textContent = msg;
    el.appendChild(p);
  };

  // A compact table with a sticky header inside a height-capped scroll box,
  // the same shape as the equipment editor, so long lists cost no height.
  const makeTable = (el, heads) => {
    el.textContent = '';
    const table = document.createElement('table');
    table.className = 'rom-equip-table rom-edit-table';
    const thead = document.createElement('thead');
    const hr = document.createElement('tr');
    for (const h of heads) {
      const th = document.createElement('th');
      th.textContent = h;
      hr.appendChild(th);
    }
    thead.appendChild(hr);
    table.appendChild(thead);
    const tbody = document.createElement('tbody');
    table.appendChild(tbody);
    el.appendChild(table);
    return tbody;
  };
  const cell = (row, child) => {
    const td = document.createElement('td');
    if (typeof child === 'string') td.textContent = child; else if (child) td.appendChild(child);
    row.appendChild(td);
    return td;
  };

  // Highlight rows that will patch, so "what did I change" is scannable.
  const markEdited = (input) => {
    const row = input.closest('.rom-edit-row');
    if (row) row.classList.toggle('is-edited', input.value.trim() !== '');
  };

  function renderFish(prizes) {
    fishRowsEl.textContent = '';
    if (!prizes || !prizes.length) {
      showEmpty(fishRowsEl, 'No fishing prize rows found on this disc.');
      return;
    }
    // One row per distinct prize item: the patch is keyed by item id and a
    // reprice hits every venue row granting that item, so a per-venue split
    // would promise control the patcher does not have.
    const byItem = new Map();
    for (const p of prizes) {
      const cur = byItem.get(p.item) || { item: p.item, name: p.name, venues: [] };
      cur.venues.push(p);
      byItem.set(p.item, cur);
    }
    const venueName = (page) => (page === 0 ? 'Buma' : 'Vidna');
    const tbody = makeTable(fishRowsEl, ['Prize', 'Current', 'New price']);
    for (const g of byItem.values()) {
      const row = document.createElement('tr');
      row.className = 'rom-edit-row';
      const name = document.createElement('span');
      name.className = 'rom-edit-name';
      const hex = '0x' + g.item.toString(16).toUpperCase().padStart(2, '0');
      name.textContent = (g.name || `item ${hex}`) + ' ';
      const code = document.createElement('code');
      code.textContent = hex;
      name.appendChild(code);
      const cur = document.createElement('span');
      cur.className = 'rom-edit-cur';
      cur.textContent = g.venues
        .map((v) => `${venueName(v.page)} ${v.price.toLocaleString()} pts${v.one_time ? ' (one-time)' : ''}`)
        .join(' · ');
      const input = document.createElement('input');
      input.type = 'number';
      input.min = '1';
      input.max = '9999999';
      input.placeholder = String(g.venues[0].price);
      input.dataset.item = String(g.item);
      input.dataset.cur = JSON.stringify(g.venues.map((v) => v.price));
      input.addEventListener('input', () => markEdited(input));
      const edit = document.createElement('label');
      edit.className = 'rom-edit-cell';
      const unit = document.createElement('span');
      unit.className = 'rom-edit-unit';
      unit.textContent = 'pts';
      edit.appendChild(input);
      edit.appendChild(unit);
      cell(row, name);
      cell(row, cur);
      cell(row, edit);
      tbody.appendChild(row);
    }
  }

  function renderLocations(names, extra) {
    locRowsEl.textContent = '';
    if (!names || !names.length) {
      showEmpty(locRowsEl, 'No location-name table found on this disc.');
      return;
    }
    const tbody = makeTable(locRowsEl, ['Place', 'New name']);
    const addRow = (cur, index) => {
      const row = document.createElement('tr');
      row.className = 'rom-edit-row';
      const name = document.createElement('span');
      name.className = 'rom-edit-name';
      name.textContent = cur || `(slot ${index})`;
      const input = document.createElement('input');
      input.type = 'text';
      input.maxLength = maxNameLen;
      input.placeholder = 'new name (blank = keep)';
      input.spellcheck = false;
      input.autocomplete = 'off';
      // Quick-travel cells are keyed by index; the world-map-only places
      // (a label and an entry banner, but no cell) by their current name.
      if (index !== undefined) input.dataset.index = String(index);
      input.dataset.cur = cur;
      input.addEventListener('input', () => markEdited(input));
      cell(row, name);
      cell(row, input);
      tbody.appendChild(row);
    };
    names.forEach((cur, i) => addRow(cur, i));
    (extra || []).forEach((cur) => addRow(cur));
  }

  // Populate from the chosen disc. Guarded per-file so re-picking the same
  // disc is a no-op, and tolerant of an older WASM bundle without the export.
  let loadedFor = null;
  async function load() {
    const file = fileInput.files && fileInput.files[0];
    if (!file) return;
    const key = `${file.name}/${file.size}/${file.lastModified}`;
    if (key === loadedFor) return;
    try {
      setNote('Reading current prices & names from your disc ...');
      const mod = await wasm();
      if (typeof mod.read_manual_edit_tables !== 'function') {
        setNote('This patcher build cannot list disc values here - the raw lists under "Advanced" still work.');
        return;
      }
      const buf = await discBytes();
      const t = mod.read_manual_edit_tables(buf);
      maxNameLen = t.max_name_len || 23;
      renderFish(t.fishing);
      renderLocations(t.locations, t.world_map_only);
      loadedFor = key;
      setNote("Current values read from your disc. Leave a box empty to keep the disc's value.");
    } catch (e) {
      setNote('Could not read the disc tables: ' + (e && e.message ? e.message : e));
    }
  }
  fileInput.addEventListener('change', () => { load(); });

  return {
    load,
    clear() {
      for (const input of fishRowsEl.querySelectorAll('input')) {
        input.value = '';
        markEdited(input);
      }
      for (const input of locRowsEl.querySelectorAll('input')) {
        input.value = '';
        markEdited(input);
      }
    },
    // Serialize the edited rows to the raw inputs' exact syntax:
    // `0xHH=points` pairs for fishing, `index=name` lines for locations.
    collect() {
      const fail = (error) => ({ fishing: '', locations: '', error });
      const fishing = [];
      for (const input of fishRowsEl.querySelectorAll('input')) {
        const v = input.value.trim();
        if (!v) continue;
        const points = parseInt(v, 10);
        if (!Number.isFinite(points) || points < 1) {
          return fail('Fishing prize prices must be positive numbers of points.');
        }
        const curs = JSON.parse(input.dataset.cur || '[]');
        if (curs.length && curs.every((c) => c === points)) continue; // no-op
        const hex = '0x' + Number(input.dataset.item).toString(16).toUpperCase().padStart(2, '0');
        fishing.push(`${hex}=${points}`);
      }
      const locations = [];
      for (const input of locRowsEl.querySelectorAll('input')) {
        const v = input.value.trim();
        if (!v || v === input.dataset.cur) continue;
        if (!/^[\x20-\x7E]+$/.test(v)) {
          return fail(`Location name ${JSON.stringify(v)} has characters outside plain ASCII - the retail font can't draw them.`);
        }
        if (v.length > maxNameLen) {
          return fail(`Location name ${JSON.stringify(v)} is over ${maxNameLen} characters.`);
        }
        // Quick-travel cells are keyed by index; the world-map-only places
        // have no cell, so they are keyed by their current name.
        locations.push(`${input.dataset.index || input.dataset.cur}=${v}`);
      }
      return { fishing: fishing.join(', '), locations: locations.join('\n'), error: '' };
    },
  };
}

window.LegaiaRomPatcher = Object.assign(window.LegaiaRomPatcher || {}, {
  setupEquipmentEditor, setupManualTables,
});
