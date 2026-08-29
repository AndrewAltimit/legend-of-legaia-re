/* In-browser ROM patcher: drive the Track-1 randomizer (compiled to WASM) on a
 * user-supplied disc image, entirely client-side, and download the patched
 * image. Nothing is uploaded; the disc bytes never leave the browser.
 *
 * The WASM module (legaia_web_viewer) exposes `patch_rom(image, seed, lang_pack,
 * drops, encounters, encounter_scope, chests, shops, casino, steals, arts,
 * doors, door_coupling, house_doors, starting_items, door_of_wind, incense,
 * speed_chain, chicken_heart, good_luck_bell, all_warps,
 * unused_enemies, unused_items, equipment_drops, monster_stats, move_power,
 * element_affinity, spell_cost, equip_bonus, weapon_specialty, starting_level,
 * solo_strong_encounters, flee_exp, seru_trade, enemy_ally, shiny_seru,
 * jewel_fix, approach_softlock_fix, delilas_challenge, custom_items, fishing_prices, location_renames,
 * earth_egg_price, arts_powers, super_art_powers,
 * arts_ap_grants, arts_ap_costs, spirit_ap, damage_ap, enemy_stat_scale,
 * exp_scale, seru_catch_rate, delilas_party, delilas_arts_voice,
 * delilas_moves, enemy_attack_count, progress?)
 * -> Promise<{ data, summary, seed, lang }>`, `resolve_seed(str)`,
 * `validate_lang_pack(image, yaml) -> { ok, language, applied, skipped, message, report }`,
 * `export_lang_pack(image, language) -> yaml_string`, and
 * `lift_official_pack(usa_image, pal_image, fold_accents) -> { yaml, language,
 * exe, summary, tables, ... }` (the official-localization transfer: the user
 * supplies their OWN PAL disc as a second file, it is read in this tab, and the
 * lifted YAML is fed back through the normal `lang_pack` path so it gets the
 * same two-phase ordering and the same coverage report). `lang` / `report`
 * carry the per-section language-patch coverage: `{ language, applied,
 * already_applied, skipped, untranslated, sections: [{name, total, filled,
 * applied, already_applied, skipped}], reasons: [{reason, count}] }` (null
 * when no language pack was chosen).
 * The structured "Prices & names" editors ride `read_manual_edit_tables(image)
 * -> { max_name_len, locations: [name x16], world_map_only: [name], fishing:
 * [{ page, row, item, name,
 * price, one_time }] }` - the disc's own location-name slots and fishing
 * prize rows (with item names resolved from the disc's SCUS table), decoded
 * client-side after the user picks their disc so the site itself ships no game
 * text. The editors serialize back to the exact `fishing_prices` /
 * `location_renames` strings the raw (advanced) inputs feed, so the wire
 * format into patch_rom is unchanged.
 *
 * Texture replacement rides its own exports, all of them family-agnostic -
 * which texture families exist is decided by the registry on the Rust side
 * (`crate::texture_registry`), and this file only ever passes a `tier` string
 * back. `scan_textures(image, thumbMax) -> { tiers, textures }` catalogs every
 * texture the registry can reach; `decode_texture(image, tier, entry, section,
 * offset)` decodes one full-size (the path a view-only family takes);
 * `preview_texture_replace(image, tier, entry, section, offset, png, quantize)`
 * validates one swap and returns the original plus the as-encoded preview;
 * `apply_texture_replacements(image, specs, progress?) -> Promise<{ data,
 * summary }>` applies the queue (chained after patch_rom's output, or run
 * alone). Change packs use
 * `export_texture_pack(specs, name, author, note) -> String` and
 * `import_texture_pack(image, json, acceptHashMismatch)`.
 * Imports resolve relative to THIS file (site/js/), so the package at
 * site/wasm/ is `../wasm/...`. Shipped language packs are static assets under
 * site/lang/<lang>.yaml, fetched on demand (nothing is bundled into the WASM).
 *
 * patch_rom and apply_texture_replacements are async: both take an optional
 * trailing `progress(stage_index, stage_count, label)` callback and yield one
 * macrotask after each stage so the page's progress bar actually paints -
 * without that, the synchronous WASM run freezes the tab and the bar would
 * never repaint.
 */

let wasmMod = null;

async function ensureWasm(setStatus) {
  if (wasmMod) return wasmMod;
  setStatus('Loading patcher (WASM) ...');
  const v = window.LEGAIA_WASM_V || '0';
  wasmMod = await import('../wasm/legaia_web_viewer.js?v=' + v);
  await wasmMod.default(new URL('../wasm/legaia_web_viewer_bg.wasm?v=' + v, import.meta.url));
  return wasmMod;
}

function $(id) {
  return document.getElementById(id);
}

// --- Language packs ---------------------------------------------------------
// Shipped packs are static assets fetched on demand; a user-supplied pack is
// read from the file input. Either way the result is a YAML string handed to
// the WASM patcher; '' means no language patch.
const shippedPackCache = {};

async function fetchShippedPack(lang) {
  if (shippedPackCache[lang] !== undefined) return shippedPackCache[lang];
  // Resolve relative to this JS file's directory (site/js/ -> site/lang/).
  const url = new URL(`../lang/${lang}.yaml`, import.meta.url).href;
  const res = await fetch(url);
  if (!res.ok) throw new Error(`could not load ${lang}.yaml (${res.status})`);
  const text = await res.text();
  shippedPackCache[lang] = text;
  return text;
}

function readFileText(file) {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(r.result);
    r.onerror = () => reject(r.error || new Error('read failed'));
    r.readAsText(file);
  });
}

// The pack lifted from the user's own PAL disc this session (YAML string), or
// null. Held in memory only - it carries the official localized script, so it
// is never persisted and only leaves the tab if the user downloads it.
let liftedPack = null;

// The YAML for the currently-selected language, or '' for none. `customFile`
// is the <input type=file> for an imported pack.
async function resolveLangPack(langSel, customFile) {
  const v = langSel.value;
  if (!v) return '';
  if (v === '__custom') {
    const f = customFile.files && customFile.files[0];
    if (!f) throw new Error('choose a pack .yaml file (or pick a language)');
    return readFileText(f);
  }
  if (v === '__official') {
    if (!liftedPack) {
      throw new Error('read the official text from your PAL disc first (button above)');
    }
    return liftedPack;
  }
  return fetchShippedPack(v);
}

function triggerDownload(bytes, filename) {
  const blob = new Blob([bytes], { type: 'application/octet-stream' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  // Revoke a tick later so the download has started.
  setTimeout(() => URL.revokeObjectURL(url), 4000);
}

function patchedName(original, seed) {
  const base = (original || 'disc.bin').replace(/\.bin$/i, '');
  return `${base}.legaia-patcher-${seed}.bin`;
}

// Render the per-section language coverage block from patch_rom's `lang`
// report (or validate_lang_pack's `report`): how much of the pack landed,
// per section, and why the rest was skipped. '' when no language was chosen.
function langCoverageText(lang) {
  if (!lang) return '';
  const done = lang.applied + lang.already_applied;
  const lines = [
    '',
    `language coverage (${lang.language}): ${done} line(s) patched, ` +
    `${lang.skipped} skipped, ${lang.untranslated} not in the pack (stay English)`,
  ];
  for (const s of (lang.sections || [])) {
    if (!s.filled) continue;
    const ok = s.applied + s.already_applied;
    lines.push(`  ${s.name}: ${ok}/${s.filled} applied` + (s.skipped ? ` (${s.skipped} skipped)` : ''));
  }
  for (const r of (lang.reasons || [])) {
    lines.push(`  ${r.count} skipped: ${r.reason}`);
  }
  return lines.join('\n') + '\n';
}

// A .cue for the patched .bin. Legend of Legaia (USA) is a single-track
// MODE2/2352 disc, so the cue is fixed except for the FILE line, which must
// reference the patched .bin's name. Emulators (mednafen et al.) load the .cue
// and error if it points at a missing file, so we ship a matching one.
function cueFor(binName) {
  return `FILE "${binName}" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n`;
}

// --- Info tooltips ----------------------------------------------------------
// Every `.info-tip` is a "?" bead whose `.tip-pop` popover opens on hover /
// keyboard focus (pure CSS) and toggles on click or Enter/Space (for touch),
// with Escape and outside-clicks closing any pinned tip. Delegated on the
// document so the JS-built editor rows get the behavior for free. The bead is
// a span (not a <button>) so a tip may carry links, and being interactive
// content it never forwards a click to the checkbox of a wrapping <label>.
function setupInfoTips() {
  const openTips = () => document.querySelectorAll('.info-tip.is-open');
  const close = (tip) => {
    tip.classList.remove('is-open');
    tip.setAttribute('aria-expanded', 'false');
  };
  // Popovers center under the bead by default; near a viewport edge they pin
  // to the bead's side instead so they stay readable.
  const position = (tip) => {
    tip.classList.remove('tip-align-right', 'tip-align-left');
    const r = tip.getBoundingClientRect();
    const half = Math.min(384, window.innerWidth * 0.78) / 2;
    const mid = r.left + r.width / 2;
    if (mid + half > window.innerWidth - 12) tip.classList.add('tip-align-right');
    else if (mid - half < 12) tip.classList.add('tip-align-left');
  };
  const toggle = (tip) => {
    const open = !tip.classList.contains('is-open');
    openTips().forEach((t) => { if (t !== tip) close(t); });
    tip.classList.toggle('is-open', open);
    tip.setAttribute('aria-expanded', String(open));
    if (open) position(tip);
  };
  document.addEventListener('click', (e) => {
    const tip = e.target.closest('.info-tip');
    if (!tip) {
      openTips().forEach(close);
      return;
    }
    if (e.target.closest('a')) return; // links inside an open tip stay links
    // Inside a <summary> or <label>, a plain click would also toggle the
    // group / checkbox - the tip click is only ever about the tip.
    e.preventDefault();
    e.stopPropagation();
    toggle(tip);
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      openTips().forEach(close);
      return;
    }
    const tip = e.target.closest && e.target.closest('.info-tip');
    if (tip && (e.key === 'Enter' || e.key === ' ')) {
      e.preventDefault();
      toggle(tip);
    }
  });
  // Hover / focus opens via CSS; JS only fixes the edge alignment.
  document.addEventListener('mouseover', (e) => {
    const tip = e.target.closest && e.target.closest('.info-tip');
    if (tip && !tip.classList.contains('is-open')) position(tip);
  });
  document.addEventListener('focusin', (e) => {
    const tip = e.target.closest && e.target.closest('.info-tip');
    if (tip) position(tip);
  });
}


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
  // A d-pad glyph in the shape of the retail Arts-input pennants: four tags
  // around a hub, the priced command in dark orange and drawn `cost - 6`
  // deep on the same scale the game uses (retail 30 -> 24 px pennant), so a
  // typed cost visibly grows or shrinks it.
  const SVG_NS = 'http://www.w3.org/2000/svg';
  const pennantPoints = (dir, len) => {
    // A tag from the hub edge (r = 4) outward by `len`, 6 wide, pointed tip.
    const c = 13, r = 4, w = 3, tip = 2.5;
    const pts = [[r, -w], [r + len - tip, -w], [r + len, 0], [r + len - tip, w], [r, w]];
    const rot = { Right: [1, 0, 0, 1], Left: [-1, 0, 0, -1], Up: [0, -1, 1, 0], Down: [0, 1, -1, 0] }[dir] || [1, 0, 0, 1];
    return pts.map(([x, y]) => `${(c + rot[0] * x + rot[1] * y).toFixed(1)},${(c + rot[2] * x + rot[3] * y).toFixed(1)}`).join(' ');
  };
  const pennantLen = (cost) => Math.max(3, Math.min(9, (Number(cost) - 6) / 4));
  const dirGlyph = (cmd, cost) => {
    const svg = document.createElementNS(SVG_NS, 'svg');
    svg.setAttribute('viewBox', '0 0 26 26');
    svg.setAttribute('class', 'rom-equip-dpad');
    svg.setAttribute('aria-hidden', 'true');
    svg.dataset.cmd = cmd;
    for (const d of ['Up', 'Left', 'Right', 'Down']) {
      const poly = document.createElementNS(SVG_NS, 'polygon');
      poly.setAttribute('points', pennantPoints(d, d === cmd ? pennantLen(cost) : 4));
      poly.setAttribute('class', d === cmd ? 'is-priced' : '');
      svg.appendChild(poly);
    }
    const hub = document.createElementNS(SVG_NS, 'circle');
    hub.setAttribute('cx', '13'); hub.setAttribute('cy', '13'); hub.setAttribute('r', '2.6');
    svg.appendChild(hub);
    return svg;
  };
  const sizeGlyph = (svg, cost) => {
    const poly = svg && svg.querySelector('polygon.is-priced');
    if (poly) poly.setAttribute('points', pennantPoints(svg.dataset.cmd, pennantLen(cost)));
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
    for (const it of items) {
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

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

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
    for (const g of byItem.values()) {
      const row = document.createElement('label');
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
      const unit = document.createElement('span');
      unit.className = 'rom-edit-unit';
      unit.textContent = 'pts';
      row.appendChild(name);
      row.appendChild(cur);
      row.appendChild(input);
      row.appendChild(unit);
      fishRowsEl.appendChild(row);
    }
  }

  function renderLocations(names, extra) {
    locRowsEl.textContent = '';
    if (!names || !names.length) {
      showEmpty(locRowsEl, 'No location-name table found on this disc.');
      return;
    }
    names.forEach((cur, i) => {
      const row = document.createElement('label');
      row.className = 'rom-edit-row';
      const name = document.createElement('span');
      name.className = 'rom-edit-name';
      name.textContent = cur || `(slot ${i})`;
      const input = document.createElement('input');
      input.type = 'text';
      input.maxLength = maxNameLen;
      input.placeholder = 'new name (blank = keep)';
      input.spellcheck = false;
      input.autocomplete = 'off';
      input.dataset.index = String(i);
      input.dataset.cur = cur;
      input.addEventListener('input', () => markEdited(input));
      row.appendChild(name);
      row.appendChild(input);
      locRowsEl.appendChild(row);
    });
    // Places with a world-map label and an entry banner but no quick-travel
    // cell. Same editor, keyed by the current name instead of a cell index.
    (extra || []).forEach((cur) => {
      const row = document.createElement('label');
      row.className = 'rom-edit-row';
      const name = document.createElement('span');
      name.className = 'rom-edit-name';
      name.textContent = cur;
      const input = document.createElement('input');
      input.type = 'text';
      input.maxLength = maxNameLen;
      input.placeholder = 'new name (blank = keep)';
      input.spellcheck = false;
      input.autocomplete = 'off';
      input.dataset.cur = cur;
      input.addEventListener('input', () => markEdited(input));
      row.appendChild(name);
      row.appendChild(input);
      locRowsEl.appendChild(row);
    });
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

// --- Tactical-Art override builder ------------------------------------------
// A friendly per-art picker over the WASM params the raw text inputs feed
// (`arts_powers` = combo=powerbyte, `arts_ap_grants` / `arts_ap_costs` =
// [character:]combo=amount). The table mirrors the disc's SCUS arts table (what
// `legaia-patcher arts` lists): per character, the arts-table display index `i`
// (the AP config row *within that character's block* - the table is keyed by
// (character, row), so an AP change never spills onto another character's art),
// the button combo `k` (the matcher key), the menu AP cost, and the current
// per-hit damage multipliers `h` for context. Names / combos / AP costs are the
// same curated walkthrough data the site's arts page ships. Miracle Arts are
// left out: their table rows are not combo-addressable, so neither feature can
// target them.
//
// Damage overrides are still keyed by combo alone (they rewrite the shared art
// record's power bytes), so those DO carry over to another character with the
// same combo - which is why only the damage note mentions collateral.
const ART_TABLE = [
  { c: 'Vahn', i: 1, n: 'Burning Flare', k: 'RDLDL', ap: 50, h: '20/22/28/28' },
  { c: 'Vahn', i: 2, n: 'Fire Blow', k: 'RRDL', ap: 40, h: '22/28/28' },
  { c: 'Vahn', i: 3, n: 'Tornado Flame', k: 'RRL', ap: 30, h: '22/28' },
  { c: 'Vahn', i: 4, n: 'Cyclone', k: 'DUUU', ap: 24, h: '18/18' },
  { c: 'Vahn', i: 5, n: 'Hurricane', k: 'UUDD', ap: 24, h: '18/18' },
  { c: 'Vahn', i: 6, n: 'PK Combo', k: 'DUUL', ap: 24, h: '18/18' },
  { c: 'Vahn', i: 7, n: 'Spin Combo', k: 'UDRL', ap: 24, h: '18/18' },
  { c: 'Vahn', i: 8, n: 'Pyro Pummel', k: 'LRUL', ap: 24, h: '18/18' },
  { c: 'Vahn', i: 9, n: 'Cross-Kick', k: 'DDDU', ap: 24, h: '18/18' },
  { c: 'Vahn', i: 10, n: 'Power Punch', k: 'LLD', ap: 18, h: '22' },
  { c: 'Vahn', i: 11, n: 'Slash Kick', k: 'UDL', ap: 18, h: '22' },
  { c: 'Vahn', i: 12, n: 'Somersault', k: 'UDU', ap: 18, h: '20' },
  { c: 'Vahn', i: 13, n: 'Charging Scorch', k: 'DRU', ap: 18, h: '22' },
  { c: 'Vahn', i: 14, n: 'Hyper Elbow', k: 'LRL', ap: 18, h: '20' },
  { c: 'Noa', i: 1, n: 'Hurricane Kick', k: 'LUUUUDR', ap: 70, h: '20' },
  { c: 'Noa', i: 4, n: 'Vulture Blade', k: 'LLRLR', ap: 50, h: '18/28/18/28' },
  { c: 'Noa', i: 5, n: 'Frost Breath', k: 'LLRR', ap: 40, h: '12/12/12/28' },
  { c: 'Noa', i: 6, n: 'Tempest Break', k: 'RRLUUU', ap: 36, h: '18/18/18/18' },
  { c: 'Noa', i: 7, n: 'Rushing Gale', k: 'UULDR', ap: 30, h: '18/18/18' },
  { c: 'Noa', i: 8, n: 'Tough Love', k: 'DUDLR', ap: 30, h: '12' },
  { c: 'Noa', i: 9, n: 'Swan Driver', k: 'DUUU', ap: 24, h: '18/18' },
  { c: 'Noa', i: 10, n: 'Bird Step', k: 'DDDU', ap: 24, h: '18/18' },
  { c: 'Noa', i: 11, n: 'Dolphin Attack', k: 'RRLR', ap: 24, h: '18/18' },
  { c: 'Noa', i: 12, n: 'Mirage Lancer', k: 'RRUU', ap: 24, h: '18/18' },
  { c: 'Noa', i: 13, n: 'Blizzard Bash', k: 'RLD', ap: 18, h: '22' },
  { c: 'Noa', i: 14, n: 'Sonic Javelin', k: 'RDR', ap: 18, h: '20' },
  { c: 'Noa', i: 15, n: 'Acrobatic Blitz', k: 'UDD', ap: 18, h: '22' },
  { c: 'Noa', i: 16, n: 'Lizard Tail', k: 'UDU', ap: 18, h: '20' },
  { c: 'Gala', i: 1, n: 'Explosive Fist', k: 'RRLLL', ap: 50, h: '20/22/28/28' },
  { c: 'Gala', i: 2, n: 'Lightning Storm', k: 'RRUL', ap: 40, h: '28/22/28' },
  { c: 'Gala', i: 3, n: 'Thunder Punch', k: 'RRL', ap: 30, h: '22/28' },
  { c: 'Gala', i: 4, n: 'Bull Horns', k: 'LURDL', ap: 30, h: '18/18/18' },
  { c: 'Gala', i: 5, n: 'Electro Thrash', k: 'ULDRL', ap: 30, h: '18/18/18' },
  { c: 'Gala', i: 6, n: 'Neo Raising', k: 'LLRUL', ap: 30, h: '18/18/18' },
  { c: 'Gala', i: 7, n: 'Black Rain', k: 'ULDD', ap: 24, h: '18/18' },
  { c: 'Gala', i: 8, n: 'Side Kick', k: 'DDUU', ap: 24, h: '18/18' },
  { c: 'Gala', i: 9, n: 'Head-Splitter', k: 'LUU', ap: 18, h: '22' },
  { c: 'Gala', i: 10, n: 'Guillotine', k: 'LUL', ap: 18, h: '20' },
  { c: 'Gala', i: 11, n: 'Back Punch', k: 'LRL', ap: 18, h: '20' },
  { c: 'Gala', i: 12, n: 'Ironhead', k: 'UDD', ap: 18, h: '22' },
  { c: 'Gala', i: 13, n: 'Battering Ram', k: 'LRD', ap: 18, h: '22' },
  { c: 'Gala', i: 14, n: 'Flying Knee Attack', k: 'DUL', ap: 18, h: '22' },
];

const ARROW = { L: '←', R: '→', D: '↓', U: '↑' };

// The fifteen Super Arts - the per-character finishers a chain of ordinary arts
// triggers. They sit in the same picker as the regular arts, but they are a
// different kind of thing and only one of the two override features can reach
// them:
//
//  * **Damage works.** A Super Art has its own art record in the character's
//    player battle file (addressed by its finisher action constant `f`), with
//    the same per-strike power bytes every regular art has. That is what
//    `--super-art-power NAME=VALUE` edits, keyed by name.
//  * **AP belongs to the chain, not to the Super.** A Super Art does cost the
//    player AP - that is the chain arts being paid for, which is condition 2
//    of the trigger (every art in the chain must already be known and paid).
//    The Super itself is free: retail computes an art's cost as
//    `multiplier x command_count` keyed on its position in the character's
//    arts list, and a Super Art has no position in that list, so there is no
//    per-Super number anywhere to edit. The lever is real but it lives on the
//    chain arts, which are rows in this same picker - so the row's AP control
//    is disabled and names them instead of just refusing.
//
// `chain` is the ordered list of named arts whose combination fires it - what a
// regular row shows as a button combo. `h` is the retail per-hit multiplier for
// context, same column the regular table carries.
const SUPER_ART_TABLE = [
  { c: 'Vahn', f: 0x2B, n: 'Tri-Somersault', chain: ['Somersault', 'Cyclone', 'Somersault'], h: '12' },
  { c: 'Vahn', f: 0x2C, n: 'Maximum Blow', chain: ['Charging Scorch', 'Slash Kick', 'Power Punch'], h: '28' },
  { c: 'Vahn', f: 0x2D, n: 'Fire Tackle', chain: ['Hyper Elbow', 'Power Punch', 'Charging Scorch'], h: '28' },
  { c: 'Vahn', f: 0x2E, n: 'Power Slash', chain: ['Charging Scorch', 'Somersault', 'Slash Kick'], h: '28' },
  { c: 'Vahn', f: 0x2F, n: 'Rolling Combo', chain: ['Spin Combo', 'Power Punch', 'PK Combo'], h: '12/12' },
  { c: 'Noa', f: 0x2E, n: 'Triple Lizard', chain: ['Bird Step', 'Swan Driver', 'Lizard Tail'], h: '12' },
  { c: 'Noa', f: 0x2F, n: 'Super Javelin', chain: ['Rushing Gale', 'Sonic Javelin'], h: '28' },
  { c: 'Noa', f: 0x30, n: 'Super Tempest', chain: ['Dolphin Attack', 'Tempest Break'], h: '12/12/12/12' },
  { c: 'Noa', f: 0x31, n: 'Love You', chain: ['Mirage Lancer', 'Lizard Tail', 'Tough Love'], h: '12/12/12/12' },
  { c: 'Noa', f: 0x32, n: 'Dragon Fangs', chain: ['Lizard Tail', 'Swan Driver', 'Acrobatic Blitz'], h: '12/12/12/12' },
  { c: 'Gala', f: 0x2B, n: 'Back Punch x3', chain: ['Ironhead', 'Flying Knee Attack', 'Back Punch'], h: '12' },
  { c: 'Gala', f: 0x2C, n: 'Super Ironhead', chain: ['Flying Knee Attack', 'Head-Splitter', 'Ironhead'], h: '28' },
  { c: 'Gala', f: 0x2D, n: 'Rushing Crush', chain: ['Battering Ram', 'Flying Knee Attack', 'Head-Splitter'], h: '28' },
  { c: 'Gala', f: 0x2E, n: "Heaven's Drop", chain: ['Flying Knee Attack', 'Head-Splitter', 'Black Rain'], h: '12/12/12/12' },
  { c: 'Gala', f: 0x2F, n: 'Neo Static Raising', chain: ['Back Punch', 'Guillotine', 'Neo Raising'], h: '12/12/12' },
];

// --- Injected-code arena conflicts ------------------------------------------
//
// Four features hand-assemble MIPS into the same 652 bytes of verified-dead
// space in SCUS_942.54, so at most one of them can be enabled at a time. The
// guard for that used to fire only at submit time and only named the *features*
// - useless when the "feature" is two picker rows the user added seconds ago
// via the Super Art row's own "add rows for those arts" button, which nobody
// reads as a mod toggle. These helpers describe the conflict in terms of the
// controls that actually cause it, so the same sentence can be shown live next
// to the control and again at submit.

// Every Tactical-Art picker row currently carrying an AP override, named.
function apOverrideRows() {
  return [...document.querySelectorAll('#rom-art-rows .art-row')]
    .filter((r) => r.artControls && r.artControls.pick.value)
    .filter((r) => !superArtByPick(r.artControls.pick.value))
    .filter((r) => r.artControls.apMode.value === 'cost' || r.artControls.apMode.value === 'grant')
    .map((r) => {
      const [c, k] = r.artControls.pick.value.split(':');
      const art = ART_TABLE.find((a) => a.c === c && a.k === k);
      return { row: r, name: art ? art.n : k, character: c, mode: r.artControls.apMode.value };
    });
}

// Everything claiming the arena right now, each as { key, label, where }.
// `label` names the control the way the page labels it; `where` says how to
// turn it off, because "turn one of them off" is not actionable on its own.
function arenaClaims() {
  const out = [];
  const chk = (id) => document.getElementById(id);
  if (chk('rom-show-super-arts') && chk('rom-show-super-arts').checked) {
    out.push({
      key: 'showSuperArts',
      label: 'Show Super Arts on the in-battle move list',
      where: 'the checkbox in Gameplay',
    });
  }
  if (chk('rom-shiny-seru') && chk('rom-shiny-seru').checked) {
    out.push({ key: 'shinySeru', label: 'Shiny Seru', where: 'the checkbox in Gameplay' });
  }
  if (chk('rom-delilas-challenge') && chk('rom-delilas-challenge').checked) {
    out.push({
      key: 'delilasChallenge',
      label: 'the Delilas Challenge',
      where: 'the checkbox in Gameplay',
    });
  }
  const rows = apOverrideRows();
  const raw = [chk('rom-arts-ap-grant'), chk('rom-arts-ap-cost')]
    .some((el) => el && (el.value || '').trim());
  if (rows.length || raw) {
    const names = [...new Set(rows.map((r) => r.name))];
    const label = rows.length
      ? `${rows.length} Tactical-Art ${rows.length === 1 ? 'row' : 'rows'} set to change AP (${names.join(', ')})`
      : 'the advanced AP-override field';
    out.push({
      key: 'artsAp',
      label,
      where: rows.length
        ? 'set their AP back to "Keep original", or remove those rows'
        : 'clear the AP-override text field',
      rows,
    });
  }
  return out;
}

// The one sentence shown both live and at submit, or '' when there is no
// conflict. Only the pairs the patcher refuses outright are reported: shiny
// Seru vs the Delilas Challenge is resolved in the patcher's favour (the
// challenge wins, shiny is skipped with a note) and is not an error.
function arenaConflictMessage() {
  const claims = arenaClaims();
  const hard =
    claims.some((c) => c.key === 'showSuperArts') && claims.length > 1
      ? claims
      : claims.some((c) => c.key === 'shinySeru') && claims.some((c) => c.key === 'artsAp')
        ? claims.filter((c) => c.key === 'shinySeru' || c.key === 'artsAp')
        : null;
  if (!hard) return '';
  const [first, ...rest] = hard;
  const others = rest.map((c) => c.label).join(', and ');
  return (
    `\u201c${first.label}\u201d cannot be combined with ${others}. ` +
    'They inject hand-written code into the same 652 bytes of unused space on the disc, ' +
    'and only one of them can have it. To fix, either ' +
    rest.map((c) => c.where).join(', or ') +
    `; or turn off \u201c${first.label}\u201d (${first.where}).`
  );
}

/// A picker option value for a Super Art. Names carry no colon, so this stays
/// unambiguous against the regular rows' `Character:COMBO`.
const SUPER_PICK_PREFIX = 'super:';

// The chain arts of `sup` as ART_TABLE rows, in trigger order and deduplicated
// (Tri-Somersault fires on Somersault > Cyclone > Somersault, and Somersault is
// one adjustable art, not two). Every one of the fifteen chains resolves.
function superChainArts(sup) {
  const out = [];
  for (const name of sup.chain) {
    if (out.some((a) => a.n === name)) continue;
    const art = ART_TABLE.find((a) => a.c === sup.c && a.n === name);
    if (art) out.push(art);
  }
  return out;
}

function superArtByPick(value) {
  if (!value || !value.startsWith(SUPER_PICK_PREFIX)) return null;
  const name = value.slice(SUPER_PICK_PREFIX.length);
  return SUPER_ART_TABLE.find((a) => a.n === name) || null;
}


function comboArrows(k) {
  return k.split('').map((ch) => ARROW[ch] || ch).join('');
}

// Damage-tier choices: the power byte's five per-hit multipliers (upper-facet
// encodings 0x0C..0x10) plus "no damage". Finer facet control stays available
// through the raw CLI-syntax input.
const DMG_TIERS = [
  { v: '', label: 'Keep original' },
  { v: '0', label: 'No damage at all' },
  { v: '0x0C', label: '×12 per hit - weakest' },
  { v: '0x0D', label: '×18 per hit - weak' },
  { v: '0x0E', label: '×20 per hit - medium' },
  { v: '0x0F', label: '×22 per hit - strong' },
  { v: '0x10', label: '×28 per hit - strongest' },
];

function artByCombo(combo) {
  return ART_TABLE.filter((a) => a.k === combo);
}

// Build one override row's DOM. `onChange` re-renders the row's effect note.
function makeArtRow(onRemove, onAddChain) {
  const row = document.createElement('div');
  row.className = 'art-row';

  const main = document.createElement('div');
  main.className = 'art-row-main';

  const mkField = (labelText, control) => {
    const f = document.createElement('label');
    f.className = 'art-field';
    const s = document.createElement('span');
    s.textContent = labelText;
    f.appendChild(s);
    f.appendChild(control);
    return f;
  };

  const pick = document.createElement('select');
  pick.className = 'art-pick';
  const ph = document.createElement('option');
  ph.value = '';
  ph.textContent = 'Choose an art ...';
  pick.appendChild(ph);
  for (const ch of ['Vahn', 'Noa', 'Gala']) {
    const g = document.createElement('optgroup');
    g.label = ch;
    for (const a of ART_TABLE.filter((x) => x.c === ch)) {
      const o = document.createElement('option');
      o.value = `${a.c}:${a.k}`;
      o.textContent = `${a.n}  ${comboArrows(a.k)}  (${a.ap} AP)`;
      g.appendChild(o);
    }
    pick.appendChild(g);
    // The character's five Super Arts, as their own visibly-separate group so
    // the fifteen read as a set rather than as more entries in the arts list.
    const sg = document.createElement('optgroup');
    sg.label = `${ch} - Super Arts`;
    for (const a of SUPER_ART_TABLE.filter((x) => x.c === ch)) {
      const o = document.createElement('option');
      o.value = `${SUPER_PICK_PREFIX}${a.n}`;
      o.textContent = `${a.n}  (Super Art - damage only)`;
      sg.appendChild(o);
    }
    pick.appendChild(sg);
  }

  const apMode = document.createElement('select');
  apMode.className = 'art-ap';
  for (const [v, t] of [['keep', 'Keep original'], ['cost', 'Costs AP'], ['grant', 'Gives AP back']]) {
    const o = document.createElement('option');
    o.value = v;
    o.textContent = t;
    apMode.appendChild(o);
  }
  // A Super Art carries no AP number of its own, so its rows select this and
  // the control is disabled. It reads as a pointer rather than a refusal: the
  // AP a player spends to fire a Super IS real, it is the chain arts' AP, and
  // those are rows in this same picker.
  const apNa = document.createElement('option');
  apNa.value = 'na';
  apNa.textContent = 'Paid by the chain arts';
  apNa.hidden = true;
  apMode.appendChild(apNa);

  // One amount box for both modes. The encoded range is the same either way:
  // the injected config table stores a signed byte per (character, art row) and
  // `0` is its "leave at retail" value, so 1..100 is the whole usable span -
  // 100 is the AP gauge's own cap (an art can never cost or grant past a full
  // gauge) and 0 is unavailable rather than "free".
  const amt = document.createElement('input');
  amt.type = 'number';
  amt.className = 'art-amt';
  amt.min = '1';
  amt.max = '100';
  amt.value = '10';
  amt.inputMode = 'numeric';
  const amtSign = document.createElement('span');
  amtSign.className = 'art-amt-sign';
  const amtUnit = document.createElement('span');
  amtUnit.className = 'art-amt-unit';
  const amtWrap = document.createElement('span');
  amtWrap.className = 'art-amt-wrap';
  amtWrap.appendChild(amtSign);
  amtWrap.appendChild(amt);
  amtWrap.appendChild(amtUnit);

  const dmg = document.createElement('select');
  dmg.className = 'art-dmg';
  for (const t of DMG_TIERS) {
    const o = document.createElement('option');
    o.value = t.v;
    o.textContent = t.label;
    dmg.appendChild(o);
  }

  const remove = document.createElement('button');
  remove.type = 'button';
  remove.className = 'art-remove';
  remove.textContent = '✕ Remove';
  remove.addEventListener('click', onRemove);

  // Super Art rows only: turn the named chain arts into rows you can actually
  // edit, so the note's "adjust the AP of X, Y" is one click rather than a
  // scavenger hunt through the picker.
  const chainBtn = document.createElement('button');
  chainBtn.type = 'button';
  chainBtn.className = 'art-chain-add';
  chainBtn.textContent = '+ Add rows for those arts';
  chainBtn.hidden = true;
  chainBtn.addEventListener('click', () => {
    const sup = superArtByPick(pick.value);
    if (sup && onAddChain) onAddChain(sup, row);
  });

  const apField = mkField('AP', apMode);
  main.appendChild(mkField('Art', pick));
  main.appendChild(apField);
  main.appendChild(mkField('Amount', amtWrap));
  main.appendChild(mkField('Damage', dmg));
  main.appendChild(chainBtn);
  main.appendChild(remove);

  const note = document.createElement('div');
  note.className = 'art-row-note';

  row.appendChild(main);
  row.appendChild(note);

  const refresh = () => {
    const sup = superArtByPick(pick.value);
    // A Super Art row: damage behaves exactly as a regular row's, the AP
    // controls are switched off, and the note says why.
    if (sup) {
      if (apMode.value !== 'na') apMode.value = 'na';
      apMode.disabled = true;
      apField.classList.add('art-field-na');
      const chainArts = superChainArts(sup);
      const chainNames = chainArts.map((a) => a.n).join(', ');
      apField.title = `A Super Art costs no AP of its own - the chain arts pay it. Adjust ${chainNames} instead.`;
      amtWrap.parentElement.hidden = true;
      // The button is only useful while some chain art is still missing a row.
      // The row is still detached on its first refresh (makeArtRow calls this
      // before the caller appends it), so scope the lookup defensively.
      const siblings = row.parentElement ? [...row.parentElement.querySelectorAll('.art-row')] : [];
      const have = new Set(siblings.filter((r) => r.artControls).map((r) => r.artControls.pick.value));
      const wanted = chainArts.filter((a) => !have.has(`${a.c}:${a.k}`));
      // Adding AP rows for the chain is exactly what collides with the
      // move-list toggle, so say it here rather than only at submit.
      const listOn = !!(document.getElementById('rom-show-super-arts') || {}).checked;
      chainBtn.hidden = !onAddChain;
      chainBtn.disabled = wanted.length === 0;
      chainBtn.title = wanted.length
        ? (listOn
          ? `Adds a row for ${wanted.map((a) => a.n).join(', ')}. An AP override cannot be combined with "Show Super Arts on the in-battle move list" - untick that first, or leave these rows on "Keep original".`
          : `Add a row for ${wanted.map((a) => a.n).join(', ')} so you can set the AP you pay to set this up.`)
        : `${chainNames} already have rows above.`;
      const parts = [
        `${sup.c}'s ${sup.n} is a Super Art: it fires when ${sup.chain.join(' > ')} are chained in that order, so it has no button combo of its own.`,
        `A Super Art costs no AP of its own - the chain arts pay it. To change what this costs to set up, adjust the AP of ${chainNames}.`,
      ];
      if (listOn) {
        parts.push(
          'Note: changing their AP cannot be combined with "Show Super Arts on the in-battle move list" - both inject code into the same unused bytes on the disc. Damage on this row is fine either way.',
        );
      }
      if (dmg.value !== '') {
        const tier = DMG_TIERS.find((t) => t.v === dmg.value);
        parts.push(`Damage: ${tier.label.toLowerCase()} (was \u00d7${sup.h}). This one is per Super Art - no other art changes.`);
      } else {
        parts.push('No change yet - pick a damage tier.');
      }
      note.textContent = parts.join(' ');
      return;
    }
    apMode.disabled = false;
    chainBtn.hidden = true;
    apField.classList.remove('art-field-na');
    apField.removeAttribute('title');
    if (apMode.value === 'na') apMode.value = 'keep';
    const sel = pick.value ? pick.value.split(':') : null;
    const art = sel ? ART_TABLE.find((a) => a.c === sel[0] && a.k === sel[1]) : null;
    const grant = apMode.value === 'grant';
    amtWrap.parentElement.hidden = apMode.value === 'keep';
    amtSign.textContent = grant ? '+' : '';
    amtUnit.textContent = 'AP each use';
    if (!art) {
      note.textContent = 'Pick an art or a Super Art to change what it does.';
      return;
    }
    const parts = [];
    const n = Math.min(100, Math.max(1, parseInt(amt.value, 10) || 10));
    if (grant) {
      parts.push(`${art.c}'s ${art.n} gives +${n} AP every use instead of costing ${art.ap} AP (usable at 0 AP, capped at 100). The pause-menu arts list will show it as 0 AP - that is the in-game marker for an art that pays you.`);
    } else if (apMode.value === 'cost') {
      parts.push(`${art.c}'s ${art.n} costs ${n} AP every use instead of ${art.ap}, and the pause-menu arts list is updated to match.`);
    }
    if (apMode.value !== 'keep') {
      parts.push("This is per character - no other character's art changes.");
    }
    if (dmg.value !== '') {
      const tier = DMG_TIERS.find((t) => t.v === dmg.value);
      parts.push(`Damage: ${tier.label.toLowerCase()} (was ×${art.h}).`);
      const twins = artByCombo(art.k).filter((a) => a.c !== art.c);
      if (twins.length) {
        parts.push('Damage is keyed by the button combo, not by character, so it also changes: ' +
          twins.map((a) => `${a.c}'s ${a.n}`).join(', ') + '.');
      }
    }
    if (!parts.length) {
      parts.push('No change yet - set AP to "Costs AP" / "Gives AP back", or pick a damage tier.');
    }
    note.textContent = parts.join(' ');
  };
  row.addEventListener('change', refresh);
  row.addEventListener('input', refresh);
  refresh();

  row.artControls = { pick, apMode, amt, dmg };
  return row;
}

// Wire the "Tactical-Art overrides" builder. Returns { clear, collect }:
// collect() serializes the rows into the same comma-separated strings the raw
// inputs use ({ power, grant, cost, error }). AP entries carry a `Character:`
// prefix because the AP config table is keyed by (character, art row); damage
// entries stay combo-only because that feature edits the shared art record.
function setupArtBuilder(container, addBtn, onEdit) {
  if (!container || !addBtn) {
    return {
      clear() {},
      collect() {
        return { power: '', grant: '', cost: '', superPower: '', error: '' };
      },
    };
  }

  // Append a row, or insert it just after `anchor` so a Super Art and the
  // chain arts it points at read as one block. `prefill` is a picker value.
  const addRow = (prefill, anchor) => {
    const row = makeArtRow(() => {
      row.remove();
      onEdit();
    }, addChainRows);
    if (anchor && anchor.parentElement === container) {
      container.insertBefore(row, anchor.nextSibling);
    } else {
      container.appendChild(row);
    }
    if (prefill) {
      row.artControls.pick.value = prefill;
      // Land on "Costs AP" - the reason the user came here is to change what
      // setting the Super up costs, and "Keep original" would write nothing.
      // EXCEPT while the move-list toggle is on: an AP override cannot be
      // combined with it, and one click that silently creates a blocking
      // conflict is worse than a row the user has to arm deliberately.
      const listOn = !!(document.getElementById('rom-show-super-arts') || {}).checked;
      row.artControls.apMode.value = listOn ? 'keep' : 'cost';
      row.dispatchEvent(new Event('change', { bubbles: true }));
    }
    return row;
  };

  // "+ Add rows for those arts" on a Super Art row: one row per chain art that
  // does not have one yet, inserted under the Super in trigger order.
  const addChainRows = (sup, anchor) => {
    const have = new Set(
      [...container.querySelectorAll('.art-row')].map((r) => r.artControls.pick.value),
    );
    let after = anchor;
    for (const art of superChainArts(sup)) {
      const value = `${art.c}:${art.k}`;
      if (have.has(value)) continue;
      have.add(value);
      after = addRow(value, after);
    }
    // Every row's note names the chain arts that now have rows, so refresh the
    // ones that were already on screen too.
    for (const r of container.querySelectorAll('.art-row')) {
      if (r.artControls) r.dispatchEvent(new Event('change', { bubbles: true }));
    }
    onEdit();
  };

  addBtn.addEventListener('click', () => {
    addRow();
    onEdit();
  });

  return {
    clear() {
      container.textContent = '';
    },
    collect() {
      const power = [];
      const grant = [];
      const cost = [];
      const superPower = [];
      const seenPower = new Set();
      const seenAp = new Set();
      const seenSuper = new Set();
      const fail = (error) => ({ power: '', grant: '', cost: '', superPower: '', error });
      for (const row of container.querySelectorAll('.art-row')) {
        const { pick, apMode, amt, dmg } = row.artControls;
        if (!pick.value) continue;
        // Super Art rows carry damage only - they have no AP cell to read, and
        // they serialize by name into `super_art_powers` rather than by combo.
        const sup = superArtByPick(pick.value);
        if (sup) {
          if (dmg.value !== '') {
            if (seenSuper.has(sup.n)) {
              return fail(`${sup.n} has two damage rows - remove the duplicate.`);
            }
            seenSuper.add(sup.n);
            superPower.push(`${sup.n}=${dmg.value}`);
          }
          continue;
        }
        const [chName, combo] = pick.value.split(':');
        const art = ART_TABLE.find((a) => a.c === chName && a.k === combo);
        if (apMode.value !== 'keep') {
          // Keyed per (character, combo): the same combo on two characters is
          // two independent entries, so only an exact repeat is a duplicate.
          const key = `${chName}:${combo}`;
          if (seenAp.has(key)) {
            return fail(`${chName}'s ${art.n} has two AP rows - remove the duplicate.`);
          }
          seenAp.add(key);
          const n = Math.min(100, Math.max(1, parseInt(amt.value, 10) || 10));
          (apMode.value === 'grant' ? grant : cost).push(`${chName}:${combo}=${n}`);
        }
        if (dmg.value !== '') {
          if (seenPower.has(combo)) {
            return fail(`${art.n} has two damage rows - remove the duplicate.`);
          }
          seenPower.add(combo);
          power.push(`${combo}=${dmg.value}`);
        }
      }
      return {
        power: power.join(', '),
        grant: grant.join(', '),
        cost: cost.join(', '),
        superPower: superPower.join(', '),
        error: '',
      };
    },
  };
}

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

  const PAGE = 60;
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
    sel = { row: t, origImg: null, pngBytes: null };
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
    await refresh();
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
        buf, t.tier, t.entry, t.section, t.offset, png, quantizeChk.checked);
      sel.origImg = r.original;
      drawRgba(origCanvas, r.original);
      if (!sel.pngBytes) {
        setVerdict('Download the original, edit it (keep the size!), then choose your PNG above.');
        return;
      }
      if (r.preview) drawRgba(newCanvas, r.preview);
      if (r.ok) {
        const bits = ['Valid - ready to add.'];
        if (r.new_palette_entries) bits.push(`${r.new_palette_entries} new palette color(s).`);
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

  // Download the selected texture's full-size decode as an editable PNG.
  exportBtn.addEventListener('click', () => {
    if (!sel || !sel.origImg) return;
    const c = document.createElement('canvas');
    drawRgba(c, sel.origImg);
    const t = sel.row;
    const name = t.tier === 'save-icon'
      ? `legaia-save-icon-slot${t.section}.png`
      : `legaia-tex-${t.tier}-${t.entry < 0 ? 'gap' : 'e' + t.entry}` +
        `${t.section >= 0 ? '-s' + t.section : ''}-0x${t.offset.toString(16)}.png`;
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

// --- Segmented (radio-group) helpers ---------------------------------------

// The checked value of a radio group, or `dflt` if none is set.
function segVal(name, dflt) {
  const el = document.querySelector(`input[name="${name}"]:checked`);
  return el ? el.value : dflt;
}

// Programmatically select a radio-group value (does not fire `change`).
function setSeg(name, value) {
  const el = document.querySelector(`input[name="${name}"][value="${value}"]`);
  if (el) el.checked = true;
}

// The enemy difficulty scale's per-stat (advanced-mode) sliders.
//
// Each entry is the stat key the patcher's own parser accepts
// (`StatScale::parse` in crates/patcher/src/monster_stats.rs), and the element
// ids are derived from it - so the strings this page emits are the same ones
// `legaia-patcher --enemy-stat-scale hp=2,attack=1.5` takes on the command line,
// and there is no separate browser vocabulary to keep in sync. The order matches
// that module's `STAT_FIELDS`, so an emitted list reads back in the order the
// patcher prints it.
//
// Simple mode is not a different feature: it sends one bare multiplier, which is
// the same type with every field equal. One parser, one planner, one set of bytes.
const SCALE_STATS = [
  ['hp', 'HP'],
  ['mp', 'MP'],
  ['attack', 'Attack'],
  ['defense_high', 'Defense, upper'],
  ['defense_low', 'Defense, lower'],
  ['intelligence', 'Intelligence'],
  ['speed', 'Speed'],
];

// The two enemy groups the scale addresses separately, in the order the emitted
// string lists them. Same widening again: a run that moves both groups together
// sends one unscoped scale, and only a genuine split sends `regular:...|boss:...`
// - so every setting written before the split existed still means what it did.
// The group keys are `ScaleProfile::parse`'s own, so nothing is translated here.
const SCALE_GROUPS = [
  ['regular', 'Random encounters'],
  ['boss', 'Bosses'],
];

// Split an emitted scale string into `{regular, boss}` bodies. The mirror of
// `ScaleProfile::parse`, and deliberately only as much of it as this page can
// itself emit plus what a preset may carry: `|`-separated `group:scale`
// segments, with a bare segment as the `all` base. Anything it can't read comes
// back as retail on both halves rather than a half-applied guess - the sliders
// are what the outgoing string is rebuilt from, so a value nothing reads would
// silently patch a different difficulty than the config asked for.
function splitScaleGroups(text) {
  const out = { regular: '', boss: '' };
  const t = String(text || '').trim();
  if (!t) return out;
  if (!t.includes(':')) return { regular: t, boss: t };
  let base = '';
  const named = {};
  for (const seg of t.split('|')) {
    const s = seg.trim();
    if (!s) continue;
    const at = s.indexOf(':');
    const group = at < 0 ? 'all' : s.slice(0, at).trim().toLowerCase();
    const body = at < 0 ? s : s.slice(at + 1).trim();
    if (group === 'all' || group === 'both' || group === 'every') base = body;
    else if (group === 'regular' || group === 'normal' || group === 'random' || group === 'common') named.regular = body;
    else if (group === 'boss' || group === 'bosses') named.boss = body;
    else return { regular: '', boss: '' }; // unknown group - don't guess
  }
  return { regular: named.regular ?? base, boss: named.boss ?? base };
}

// --- Presets ---------------------------------------------------------------
// Each preset is a full configuration: every control gets a value, so applying
// one is unambiguous. Keys map to control names / element ids below.
const PRESET_BASE = {
  drops: 'none', encounters: 'none', encounter_scope: 'scene', soloStrong: false, fleeExp: false, chests: 'none',
  shops: 'none', casino: 'none', steals: 'none', arts: 'none', doors: 'none',
  door_coupling: 'coupled', houseDoors: false, equipmentDrops: false, seruTrade: false,
  enemyAlly: false, shinySeru: false, showSuperArts: false, jewelFix: false, approachFix: false, delilasChallenge: false, customItems: false, fishingPrice: '', renameLocation: '', earthEggPrice: '', artsPower: '', superArtPower: '', artsApGrant: '', spiritAp: '', damageAp: '', enemyStatScale: '', expScale: '', seruCatchRate: '', delilasParty: '', delilasArtsVoice: 'original', delilasMoves: 'hybrid', attackCount: '',
  startingItems: 0, doorOfWind: false, incense: false,
  speedChain: false, chickenHeart: false, goodLuckBell: false,
  allWarps: false,
  unusedEnemies: false, unusedItems: false,
  monster_stats: 'none', move_power: 'none', element_affinity: 'none',
  spell_cost: 'none', equip_bonus: 'none', weaponSpecialty: false,
  startingLevel: 0,
};

// Both gameplay presets hand the player a generous start: every convenience
// item + accessory, and 5 random consumables on top. The starting LEVEL is not
// part of the bundle - each preset names its own, because the two want
// different curves (see below).
//
// `allWarps` is deliberately NOT in the bundle. It presets the visited-towns
// bitmask, so every Door-of-Wind destination is reachable before the player has
// been there - that skips the route the shuffled chests / encounters / doors are
// meant to be discovered along, and it is a route spoiler in itself. It stays a
// one-click opt-in on the toggle grid; no preset turns it on.
const STARTING_BUNDLE = {
  startingItems: 5,
  doorOfWind: true, incense: true,
  speedChain: true, chickenHeart: true, goodLuckBell: true,
};

// Equipment drops are additive - a code hook grants one extra random gear piece
// on a low per-battle chance, on top of the normal drop - so every gameplay
// preset turns them on; only vanilla leaves them off.
//
// `monster_stats` is Full Chaos only: reshuffled enemy stat blocks move fights
// off the vanilla difficulty curve that the rest of Balanced - kingdom-scope
// encounters, shuffled loot, the level-5 start - is balanced against.
//
// The two presets start at different levels on purpose. Balanced keeps the
// early curve legible: level 5 clears the opening difficulty spike without
// skipping past the fights the shuffled encounter tables are tuned around.
// Full Chaos starts at 10 because its randomized monster stats and world-scope
// encounters can seat a wildly over-levelled fight in the first region, and the
// higher floor is what keeps that survivable.
const PRESETS = {
  vanilla: { ...PRESET_BASE },
  items: {
    ...PRESET_BASE,
    drops: 'shuffle', chests: 'shuffle', shops: 'shuffle',
    casino: 'shuffle', steals: 'shuffle', equipmentDrops: true,
  },
  balanced: {
    ...PRESET_BASE,
    drops: 'shuffle', encounters: 'shuffle', encounter_scope: 'kingdom',
    soloStrong: true, fleeExp: true,
    chests: 'shuffle', steals: 'shuffle', arts: 'shuffle',
    equip_bonus: 'shuffle', equipmentDrops: true,
    seruTrade: true, enemyAlly: true, shinySeru: true, jewelFix: true, approachFix: true,
    delilasChallenge: true, customItems: true,
    ...STARTING_BUNDLE, startingLevel: 5,
  },
  chaos: {
    ...PRESET_BASE,
    drops: 'random', encounters: 'random', encounter_scope: 'world',
    soloStrong: true, fleeExp: true,
    chests: 'random', shops: 'random', casino: 'random', steals: 'random',
    arts: 'random', doors: 'random', door_coupling: 'coupled',
    houseDoors: true, unusedEnemies: true, unusedItems: true,
    monster_stats: 'random', move_power: 'random', element_affinity: 'random',
    spell_cost: 'random', equip_bonus: 'random', weaponSpecialty: true,
    equipmentDrops: true, seruTrade: true, enemyAlly: true, shinySeru: true, jewelFix: true, approachFix: true,
    delilasChallenge: true, customItems: true,
    ...STARTING_BUNDLE, startingLevel: 10,
  },
};

function init() {
  const fileInput = $('rom-file');
  // Disc-identity panel (js/disc-info.js, a classic script loaded before this
  // module): identifies the picked image (serial, region, build, PROT layout)
  // from a few sliced sectors, without reading the whole file.
  if (window.DiscInfo) window.DiscInfo.attachInput(fileInput);
  const seedInput = $('rom-seed');
  const startingItemsSel = $('rom-starting-items');
  const startingLevelSel = $('rom-starting-level');
  const doorOfWindChk = $('rom-door-of-wind');
  const doorOfWindCountInput = $('rom-door-of-wind-count');
  const incenseChk = $('rom-incense');
  const incenseCountInput = $('rom-incense-count');
  const speedChainChk = $('rom-speed-chain');
  const chickenHeartChk = $('rom-chicken-heart');
  const goodLuckBellChk = $('rom-good-luck-bell');
  const allWarpsChk = $('rom-all-warps');
  const soloStrongChk = $('rom-solo-strong');
  const fleeExpChk = $('rom-flee-exp');
  const equipmentDropsChk = $('rom-equipment-drops');
  const seruTradeChk = $('rom-seru-trade');
  const enemyAllyChk = $('rom-enemy-ally');
  const shinySeruChk = $('rom-shiny-seru');
  const showSuperArtsChk = $('rom-show-super-arts');
  const jewelFixChk = $('rom-jewel-fix');
  const approachFixChk = $('rom-approach-fix');
  const delilasChallengeChk = $('rom-delilas-challenge');
  const customItemsChk = $('rom-custom-items');
  const delilasPartySel = $('rom-delilas-party');
  const delilasArtsVoiceSel = $('rom-delilas-arts-voice');
  const delilasArtsVoiceRow = $('rom-delilas-arts-voice-row');
  const delilasMovesSel = $('rom-delilas-moves');
  const delilasMovesRow = $('rom-delilas-moves-row');
  // The arts-voice sub-option only means anything with the swap on.
  const syncDelilasArtsRow = () => {
    delilasArtsVoiceRow.hidden = !delilasPartySel.value;
    delilasMovesRow.hidden = !delilasPartySel.value;
  };
  delilasPartySel.addEventListener('change', syncDelilasArtsRow);
  const fishingPriceInput = $('rom-fishing-price');
  const renameLocationInput = $('rom-rename-location');
  const earthEggPriceInput = $('rom-earth-egg-price');
  const spiritApChk = $('rom-spirit-ap-on');
  const spiritApSlider = $('rom-spirit-ap');
  const spiritApVal = $('rom-spirit-ap-val');
  const damageApChk = $('rom-damage-ap-on');
  const damageApSlider = $('rom-damage-ap');
  const damageApVal = $('rom-damage-ap-val');
  const enemyScaleChk = $('rom-enemy-scale-on');
  const expScaleChk = $('rom-exp-scale-on');
  const expScaleSlider = $('rom-exp-scale');
  const expScaleVal = $('rom-exp-scale-val');
  const seruCatchChk = $('rom-seru-catch-on');
  const seruCatchSlider = $('rom-seru-catch');
  const seruCatchVal = $('rom-seru-catch-val');
  const attackCountChk = $('rom-attack-count-on');
  const attackCountSlider = $('rom-attack-count');
  const attackCountVal = $('rom-attack-count-val');
  // Live read-out next to each AP slider. `input` fires while dragging;
  // `change` (which drives markCustom/syncDependents) only fires on release.
  for (const [slider, out] of [[spiritApSlider, spiritApVal], [damageApSlider, damageApVal], [seruCatchSlider, seruCatchVal]]) {
    if (slider && out) slider.addEventListener('input', () => { out.textContent = slider.value; });
  }
  // The difficulty scale is a multiplier, so it reads out as "2.5x". Its step
  // is 0.1, and float steps can land on 2.5000000000000004 - fix the display
  // (and everything derived from it) to one decimal.
  const fmtScale = (v) => Number(v).toFixed(1) + 'x';
  // The EXP multiplier reads out like the difficulty scale ("2.5x").
  if (expScaleSlider && expScaleVal) {
    expScaleSlider.addEventListener('input', () => { expScaleVal.textContent = fmtScale(expScaleSlider.value); });
  }
  // So does the enemy attack-count multiplier.
  if (attackCountSlider && attackCountVal) {
    attackCountSlider.addEventListener('input', () => { attackCountVal.textContent = fmtScale(attackCountSlider.value); });
  }
  // Bind one slider + read-out pair, and return a handle the setters use.
  // Dropped silently if the markup is absent, so an older page still works.
  const bindScale = (id, key) => {
    const slider = $(id);
    const out = $(`${id}-val`);
    if (!slider || !out) return null;
    slider.addEventListener('input', () => { out.textContent = fmtScale(slider.value); });
    return { key, slider, out };
  };
  // Simple mode: one slider per enemy group. Advanced mode: one per (group,
  // stat). Both keyed by the patcher's own group and stat names, so the strings
  // this page emits are the ones `legaia-patcher --enemy-stat-scale` takes.
  const enemyScaleSimple = new Map(
    SCALE_GROUPS.map(([g]) => [g, bindScale(`rom-enemy-scale-${g}`, g)]).filter(([, f]) => f),
  );
  const enemyScaleFields = new Map(
    SCALE_GROUPS.map(([g]) => [
      g,
      SCALE_STATS.map(([key]) => bindScale(`rom-enemy-scale-${g}-${key}`, key)).filter(Boolean),
    ]),
  );
  // Set one group's per-stat sliders at once, and keep their read-outs in step.
  const setScaleFields = (group, valueFor) => {
    for (const f of enemyScaleFields.get(group) || []) {
      f.slider.value = String(valueFor(f.key));
      f.out.textContent = fmtScale(f.slider.value);
    }
  };
  const setSimpleScale = (group, value) => {
    const f = enemyScaleSimple.get(group);
    if (!f) return;
    f.slider.value = String(value);
    f.out.textContent = fmtScale(f.slider.value);
  };
  // Fourteen sliders need a way back to neutral that isn't "drag each one".
  const enemyScaleReset = $('rom-enemy-scale-reset');
  if (enemyScaleReset) {
    enemyScaleReset.addEventListener('click', () => {
      for (const [g] of SCALE_GROUPS) setScaleFields(g, () => 1);
      // A button emits `click`, not `change`, so the form-level listener that
      // normally flips the preset chip never sees this.
      markCustom();
    });
  }
  const artsPowerInput = $('rom-arts-power');
  const superArtPowerInput = $('rom-super-art-power');
  const artsApGrantInput = $('rom-arts-ap-grant');
  const artBuilder = setupArtBuilder($('rom-art-rows'), $('rom-art-add'), () => markCustom());
  const weaponSpecialtyChk = $('rom-weapon-specialty');
  const houseDoorsChk = $('rom-house-doors');
  const unusedEnemiesChk = $('rom-unused-enemies');
  const unusedItemsChk = $('rom-unused-items');
  const langSel = $('rom-lang');
  const langFileRow = $('rom-lang-file-row');
  const langFile = $('rom-lang-file');
  const langOfficialRow = $('rom-lang-official-row');
  const langPalFile = $('rom-lang-pal-file');
  const langFoldChk = $('rom-lang-fold');
  const langLiftBtn = $('rom-lang-lift');
  const langLiftSaveBtn = $('rom-lang-lift-save');
  const langValidateBtn = $('rom-lang-validate');
  const langExportBtn = $('rom-lang-export');
  const langStatusEl = $('rom-lang-status');
  const runBtn = $('rom-run');
  const statusEl = $('rom-status');
  const summaryEl = $('rom-summary');
  const progressEl = $('rom-progress');
  const progressTrack = progressEl ? progressEl.querySelector('.rom-progress-track') : null;
  const progressFill = $('rom-progress-fill');
  const progressLabel = $('rom-progress-label');
  const formEl = document.querySelector('.rom-form');
  const presetBar = $('rom-presets');
  const customChip = $('rom-preset-custom');
  if (!fileInput || !runBtn) return; // not on this page

  const setStatus = (msg, kind) => {
    statusEl.textContent = msg;
    statusEl.className = 'rom-status' + (kind ? ' rom-status-' + kind : '');
  };
  // Stage-progress callback handed to the async WASM patch entry points.
  // The WASM side yields a macrotask after each invocation, which is what
  // lets these DOM writes actually paint mid-run.
  const onPatchProgress = (idx, count, label) => {
    if (!progressEl) return;
    progressEl.hidden = false;
    const pct = count > 0 ? Math.round((idx / count) * 100) : 0;
    progressFill.style.width = pct + '%';
    if (progressTrack) progressTrack.setAttribute('aria-valuenow', String(pct));
    progressLabel.textContent = label + ' (' + (idx + 1) + '/' + count + ')';
  };
  const hidePatchProgress = () => {
    if (!progressEl) return;
    progressEl.hidden = true;
    progressFill.style.width = '0%';
    progressLabel.textContent = '';
  };
  const setLangStatus = (msg, kind) => {
    langStatusEl.textContent = msg;
    langStatusEl.className = 'rom-status' + (kind ? ' rom-status-' + kind : '');
  };

  // The custom-pack file input is only relevant when "Import my own pack" is
  // chosen; the group is opt-in and defaults to None.
  function syncLangRow() {
    if (langFileRow) langFileRow.hidden = langSel.value !== '__custom';
    if (langOfficialRow) langOfficialRow.hidden = langSel.value !== '__official';
  }
  langSel.addEventListener('change', () => { syncLangRow(); setLangStatus(''); });
  syncLangRow();

  // The current disc file's bytes, or an error if none is chosen.
  async function discBytes() {
    const file = fileInput.files && fileInput.files[0];
    if (!file) throw new Error('choose a disc image (.bin) first');
    return new Uint8Array(await file.arrayBuffer());
  }

  // Texture replacement panel (its queue is orthogonal to the presets, like
  // the language choice).
  const texture = setupTextureReplacer(() => ensureWasm(setStatus), discBytes);

  // Hover/tap tooltips + the structured "Prices & names" editors (rows filled
  // from the user's own disc when one is chosen).
  setupInfoTips();
  const manualTables = setupManualTables(() => ensureWasm(setStatus), fileInput, discBytes);
  const equipmentEditor = setupEquipmentEditor(() => ensureWasm(setStatus), fileInput, discBytes);
  const swingCostInput = $('rom-swing-cost');
  const equipOwnerInput = $('rom-equip-owner');

  // "Check pack against my disc": the same disc-measured dry run the CLI does.
  langValidateBtn.addEventListener('click', async () => {
    try {
      setLangStatus('Checking ...');
      const yaml = await resolveLangPack(langSel, langFile);
      if (!yaml) { setLangStatus('No language selected (English).'); return; }
      const mod = await ensureWasm(setStatus);
      const buf = await discBytes();
      const r = mod.validate_lang_pack(buf, yaml);
      setLangStatus(`${langSel.options[langSel.selectedIndex].text}: ${r.message}`, 'ok');
      // Per-section dry-run coverage in the summary panel (same shape as the
      // post-patch report).
      if (r.report) summaryEl.textContent = langCoverageText(r.report).trim();
    } catch (e) {
      setLangStatus('Error: ' + (e && e.message ? e.message : e), 'err');
    }
  });

  // "Read the official text from my PAL disc": the official-localization
  // transfer. The user supplies a SECOND disc they own (a PAL SCES build); it
  // is read in this tab exactly like the USA one, lifted onto USA coordinates,
  // and kept in memory as an ordinary language pack. Patching then goes through
  // the normal lang_pack path, so the ordering and the coverage report are the
  // same as for any community pack.
  //
  // A lift holds both disc images in WASM memory at once, so it is done as its
  // own call and both are dropped before the patch run re-supplies the USA disc.
  langLiftBtn.addEventListener('click', async () => {
    const palFile = langPalFile.files && langPalFile.files[0];
    if (!palFile) {
      setLangStatus('Choose your PAL disc image (.bin) first.', 'err');
      return;
    }
    langLiftBtn.disabled = true;
    try {
      setLangStatus('Reading both discs (nothing is uploaded) ...');
      const mod = await ensureWasm(setStatus);
      const usa = await discBytes();
      const pal = new Uint8Array(await palFile.arrayBuffer());
      setLangStatus('Reading the official text (this takes a moment) ...');
      await new Promise((r) => setTimeout(r, 30));
      const r = mod.lift_official_pack(usa, pal, langFoldChk.checked);
      liftedPack = r.yaml;
      langLiftSaveBtn.hidden = false;
      langLiftSaveBtn.dataset.lang = r.language;
      setLangStatus(
        `Official ${r.language.toUpperCase()} text read from ${r.exe}. ` +
        'Now press "Patch my disc" below - the coverage report will say how much of it fits.',
        'ok');
      summaryEl.textContent = r.summary || '';
    } catch (e) {
      setLangStatus('Error: ' + (e && e.message ? e.message : e), 'err');
    } finally {
      langLiftBtn.disabled = false;
    }
  });

  // Keep the lifted pack (it is the user's own disc text, so it is theirs to
  // keep - and it can be edited and re-imported through the pack path).
  langLiftSaveBtn.addEventListener('click', () => {
    if (!liftedPack) return;
    const code = langLiftSaveBtn.dataset.lang || 'xx';
    triggerDownload(new TextEncoder().encode(liftedPack), `legaia_${code}.official.yaml`);
    setLangStatus(`Downloaded legaia_${code}.official.yaml - it holds the game's script, so keep it to yourself.`, 'ok');
  });

  // Re-lifting is required when the PAL disc or the accent choice changes.
  const invalidateLift = () => {
    liftedPack = null;
    langLiftSaveBtn.hidden = true;
    setLangStatus('');
  };
  langPalFile.addEventListener('change', invalidateLift);
  langFoldChk.addEventListener('change', invalidateLift);

  // "Export a starter pack from my disc": dump a source-bearing working pack the
  // user can edit. Uses the chosen language code as the header stamp (or en).
  langExportBtn.addEventListener('click', async () => {
    try {
      setLangStatus('Exporting starter pack from your disc ...');
      const mod = await ensureWasm(setStatus);
      const buf = await discBytes();
      const code = (langSel.value && langSel.value !== '__custom') ? langSel.value : 'en';
      const yaml = mod.export_lang_pack(buf, code);
      const bytes = new TextEncoder().encode(yaml);
      triggerDownload(bytes, `legaia_${code}.working.yaml`);
      setLangStatus(`Downloaded legaia_${code}.working.yaml - fill the translation: fields and import it above.`, 'ok');
    } catch (e) {
      setLangStatus('Error: ' + (e && e.message ? e.message : e), 'err');
    }
  });

  // Apply a named preset to every control.
  function applyPreset(name) {
    const cfg = PRESETS[name];
    if (!cfg) return;
    for (const seg of ['drops', 'encounters', 'encounter_scope', 'chests',
      'shops', 'casino', 'steals', 'arts', 'doors', 'door_coupling',
      'monster_stats', 'move_power', 'element_affinity', 'spell_cost',
      'equip_bonus']) {
      setSeg(seg, cfg[seg]);
    }
    houseDoorsChk.checked = cfg.houseDoors;
    soloStrongChk.checked = cfg.soloStrong;
    fleeExpChk.checked = cfg.fleeExp;
    equipmentDropsChk.checked = cfg.equipmentDrops;
    seruTradeChk.checked = cfg.seruTrade;
    enemyAllyChk.checked = cfg.enemyAlly;
    shinySeruChk.checked = cfg.shinySeru;
    showSuperArtsChk.checked = !!cfg.showSuperArts;
    jewelFixChk.checked = cfg.jewelFix;
    approachFixChk.checked = cfg.approachFix;
    delilasChallengeChk.checked = cfg.delilasChallenge;
    customItemsChk.checked = cfg.customItems;
    delilasPartySel.value = cfg.delilasParty ?? '';
    delilasArtsVoiceSel.value = cfg.delilasArtsVoice ?? 'original';
    delilasMovesSel.value = cfg.delilasMoves ?? 'hybrid';
    syncDelilasArtsRow();
    fishingPriceInput.value = cfg.fishingPrice || '';
    renameLocationInput.value = cfg.renameLocation || '';
    earthEggPriceInput.value = cfg.earthEggPrice || '';
    spiritApChk.checked = cfg.spiritAp !== '' && cfg.spiritAp != null;
    spiritApSlider.value = String(spiritApChk.checked ? cfg.spiritAp : 32);
    spiritApVal.textContent = spiritApSlider.value;
    damageApChk.checked = cfg.damageAp !== '' && cfg.damageAp != null;
    damageApSlider.value = String(damageApChk.checked ? cfg.damageAp : 100);
    damageApVal.textContent = damageApSlider.value;
    expScaleChk.checked = cfg.expScale !== '' && cfg.expScale != null;
    expScaleSlider.value = String(expScaleChk.checked ? cfg.expScale : 1);
    expScaleVal.textContent = fmtScale(expScaleSlider.value);
    seruCatchChk.checked = cfg.seruCatchRate !== '' && cfg.seruCatchRate != null;
    seruCatchSlider.value = String(seruCatchChk.checked ? cfg.seruCatchRate : 100);
    seruCatchVal.textContent = seruCatchSlider.value;
    attackCountChk.checked = cfg.attackCount !== '' && cfg.attackCount != null;
    attackCountSlider.value = String(attackCountChk.checked ? cfg.attackCount : 1);
    attackCountVal.textContent = fmtScale(attackCountSlider.value);
    // Difficulty scale. A config value is one string carrying both groups; each
    // group's body is either a bare multiplier or a `stat=mult` list, so the
    // string itself picks the view mode - a preset that shapes individual stats
    // opens in Advanced without needing a second key.
    enemyScaleChk.checked = cfg.enemyStatScale !== '' && cfg.enemyStatScale != null;
    const scaleText = enemyScaleChk.checked ? String(cfg.enemyStatScale) : '';
    const scaleByGroup = splitScaleGroups(scaleText);
    // Advanced as soon as *either* group shapes individual stats: the two panes
    // are one control, and a mixed config must not open on the pane that would
    // drop half of it.
    const perStat = Object.values(scaleByGroup).some((v) => v.includes('='));
    setSeg('enemy_scale_mode', perStat ? 'advanced' : 'simple');
    for (const [group] of SCALE_GROUPS) {
      const body = scaleByGroup[group] || '';
      setSimpleScale(group, !body || body.includes('=') ? 1 : body);
      // Whatever the list doesn't name goes back to 1.0x, so no stale slider
      // survives a preset switch.
      const named = new Map(
        (body.includes('=') ? body.split(/[,;\s]+/) : [])
          .filter(Boolean)
          .map((tok) => tok.split('='))
          .filter((kv) => kv.length === 2)
          .map(([k, v]) => [k.trim().toLowerCase(), v.trim()]),
      );
      // `defense` is the CLI's alias for both defense halves, so expand it
      // rather than dropping it - the sliders are what the emitted string is
      // rebuilt from, and a key nothing reads would silently apply a different
      // difficulty than the config asked for. Only this one alias is honoured
      // here: a config value should otherwise use the canonical SCALE_STATS
      // keys, which are the only ones this page ever emits.
      const bothDefenses = named.get('defense') ?? named.get('def');
      if (bothDefenses !== undefined) {
        for (const half of ['defense_high', 'defense_low']) {
          if (!named.has(half)) named.set(half, bothDefenses);
        }
      }
      // A group given only a bare multiplier still has to reach the advanced
      // pane, or switching to Advanced would silently drop it back to 1.0x.
      const fallback = !body || body.includes('=') ? 1 : body;
      setScaleFields(group, (key) => named.get(key) ?? fallback);
    }
    artsPowerInput.value = cfg.artsPower || '';
    superArtPowerInput.value = cfg.superArtPower || '';
    artsApGrantInput.value = cfg.artsApGrant || '';
    artBuilder.clear();
    manualTables.clear();
    equipmentEditor.clear();
    if (swingCostInput) swingCostInput.value = '';
    if (equipOwnerInput) equipOwnerInput.value = '';
    weaponSpecialtyChk.checked = cfg.weaponSpecialty;
    startingItemsSel.value = String(cfg.startingItems);
    startingLevelSel.value = String(cfg.startingLevel);
    doorOfWindChk.checked = cfg.doorOfWind;
    incenseChk.checked = cfg.incense;
    speedChainChk.checked = cfg.speedChain;
    chickenHeartChk.checked = cfg.chickenHeart;
    goodLuckBellChk.checked = cfg.goodLuckBell;
    allWarpsChk.checked = cfg.allWarps;
    unusedEnemiesChk.checked = cfg.unusedEnemies;
    unusedItemsChk.checked = cfg.unusedItems;
    // Reflect the active preset in the chip row.
    presetBar.querySelectorAll('.rom-preset').forEach((b) => {
      b.classList.toggle('is-active', b.dataset.preset === name);
    });
    if (customChip) customChip.hidden = true;
    syncDependents();
  }

  // After a manual edit, no single preset describes the form any more.
  function markCustom() {
    presetBar.querySelectorAll('.rom-preset').forEach((b) => b.classList.remove('is-active'));
    if (customChip) customChip.hidden = false;
  }

  // Show the arena conflict the moment it exists, next to the control that
  // causes it. Submit time is the worst moment to learn that two settings are
  // incompatible - by then everything else has been configured.
  function syncArenaConflict() {
    const box = $('rom-arena-conflict');
    if (!box) return '';
    const msg = arenaConflictMessage();
    box.textContent = msg;
    box.hidden = !msg;
    const claims = msg ? arenaClaims() : [];
    const apClaim = claims.find((c) => c.key === 'artsAp');
    // Mark the exact rows, so "2 Tactical-Art rows" is something you can see.
    for (const r of document.querySelectorAll('#rom-art-rows .art-row')) {
      r.classList.remove('art-row-conflict');
    }
    if (msg && apClaim && apClaim.rows) {
      for (const { row } of apClaim.rows) row.classList.add('art-row-conflict');
    }
    // ...and the checkboxes, so every side of the conflict is visibly marked.
    const BOX = {
      showSuperArts: 'rom-show-super-arts',
      shinySeru: 'rom-shiny-seru',
      delilasChallenge: 'rom-delilas-challenge',
    };
    const lit = new Set(claims.map((c) => BOX[c.key]).filter(Boolean));
    for (const id of Object.values(BOX)) {
      const el = $(id);
      const rowEl = el && el.closest('.rom-check-row');
      if (rowEl) rowEl.classList.toggle('rom-check-conflict', lit.has(id));
    }
    return msg;
  }

  // Grey out controls that have no effect given the current state.
  function syncDependents() {
    syncArenaConflict();
    const encOn = segVal('encounters', 'none') !== 'none';
    const doorsOn = segVal('doors', 'none') !== 'none';
    const scopeRow = $('rom-scope-row');
    const couplingRow = $('rom-coupling-row');
    const soloRow = $('rom-solo-strong-row');
    if (scopeRow) scopeRow.classList.toggle('is-disabled', !encOn);
    if (couplingRow) couplingRow.classList.toggle('is-disabled', !doorsOn);
    // Solo-strong only does anything while encounters are being randomized.
    if (soloRow) soloRow.classList.toggle('is-disabled', !encOn);
    // Each AP slider only applies while its own override checkbox is on.
    const spiritRow = $('rom-spirit-ap-row');
    if (spiritRow) spiritRow.classList.toggle('is-disabled', !(spiritApChk && spiritApChk.checked));
    const damageRow = $('rom-damage-ap-row');
    if (damageRow) damageRow.classList.toggle('is-disabled', !(damageApChk && damageApChk.checked));
    const expScaleRow = $('rom-exp-scale-row');
    if (expScaleRow) expScaleRow.classList.toggle('is-disabled', !(expScaleChk && expScaleChk.checked));
    const seruCatchRow = $('rom-seru-catch-row');
    if (seruCatchRow) seruCatchRow.classList.toggle('is-disabled', !(seruCatchChk && seruCatchChk.checked));
    const attackCountRow = $('rom-attack-count-row');
    if (attackCountRow) attackCountRow.classList.toggle('is-disabled', !(attackCountChk && attackCountChk.checked));
    const enemyScaleRow = $('rom-enemy-scale-row');
    if (enemyScaleRow) enemyScaleRow.classList.toggle('is-disabled', !(enemyScaleChk && enemyScaleChk.checked));
    // The scale's two view modes: exactly one pane is ever visible, and the
    // hidden one's sliders are not read when the patch is built.
    const scaleAdvanced = segVal('enemy_scale_mode', 'simple') === 'advanced';
    const simplePane = $('rom-enemy-scale-simple-pane');
    const advancedPane = $('rom-enemy-scale-advanced-pane');
    if (simplePane) simplePane.hidden = scaleAdvanced;
    if (advancedPane) advancedPane.hidden = !scaleAdvanced;
    // Equipment drops are additive (an extra reward-routine grant), so the
    // Monster drops control stays fully live alongside them - nothing to grey.
  }

  // Preset chip clicks.
  presetBar.addEventListener('click', (e) => {
    const btn = e.target.closest('.rom-preset');
    if (!btn) return;
    applyPreset(btn.dataset.preset);
  });

  // Any manual control edit → "Custom" + re-sync dependent controls. The preset
  // buttons live in the same form but emit `click`, not `change`, and applyPreset
  // sets values programmatically (which never fires `change`), so this only runs
  // on genuine user edits.
  formEl.addEventListener('change', (e) => {
    // Seed, disc-file, the language selection and the texture panel are
    // orthogonal to the randomization config, so editing them must not flip
    // the preset to "Custom".
    if (e.target && (['rom-seed', 'rom-file', 'rom-lang', 'rom-lang-file',
      'rom-lang-pal-file', 'rom-lang-fold'].includes(e.target.id)
      || (e.target.id || '').startsWith('rom-tex-'))) return;
    markCustom();
    syncDependents();
  });

  syncDependents();

  runBtn.addEventListener('click', async () => {
    const file = fileInput.files && fileInput.files[0];
    if (!file) {
      setStatus('Choose a disc image (.bin) first.', 'err');
      return;
    }
    const drops = segVal('drops', 'none');
    const encounters = segVal('encounters', 'none');
    const encounterScope = segVal('encounter_scope', 'scene');
    const soloStrong = soloStrongChk.checked;
    const fleeExp = fleeExpChk.checked;
    const seruTrade = seruTradeChk.checked;
    const enemyAlly = enemyAllyChk.checked;
    const shinySeru = shinySeruChk.checked;
    const showSuperArts = showSuperArtsChk.checked;
    const jewelFix = jewelFixChk.checked;
    const approachFix = approachFixChk.checked;
    const delilasChallenge = delilasChallengeChk.checked;
    const customItems = customItemsChk.checked;
    // Prices & names = the structured rows serialized to the raw inputs'
    // syntax, merged with anything typed into the raw (advanced) inputs.
    const manual = manualTables.collect();
    if (manual.error) {
      setStatus(manual.error, 'err');
      return;
    }
    const fishingPrice = [manual.fishing, (fishingPriceInput.value || '').trim()]
      .filter(Boolean).join(', ');
    const renameLocation = [manual.locations, (renameLocationInput.value || '').trim()]
      .filter(Boolean).join('\n');
    const earthEggPrice = (earthEggPriceInput.value || '').trim();
    // Equipment editor rows + the raw (advanced) token lists, same syntax.
    const equipEdits = equipmentEditor.collect();
    if (equipEdits.error) {
      setStatus(equipEdits.error, 'err');
      return;
    }
    const swingCosts = [equipEdits.costs, (swingCostInput && swingCostInput.value || '').trim()]
      .filter(Boolean).join(',');
    const equipOwners = [equipEdits.owners, (equipOwnerInput && equipOwnerInput.value || '').trim()]
      .filter(Boolean).join(',');
    // AP sliders: only sent when their override checkbox is on ('' = retail).
    // Both ranges include 0 and negatives, so read the value as-is.
    const spiritAp = spiritApChk.checked ? String(spiritApSlider.value) : '';
    const damageAp = damageApChk.checked ? String(damageApSlider.value) : '';
    // Difficulty scale. Per enemy group: Simple sends one multiplier for every
    // stat; Advanced sends a `stat=mult` list naming only the stats that
    // actually move, which is the same spelling the CLI takes. Every value is
    // fixed to one decimal, because a 0.1 float step can land on
    // 2.5000000000000004 and the parser rounds to thousandths.
    //
    // The two groups then collapse where they can: equal bodies send one
    // unscoped scale (byte-identical to what this page sent before the split
    // existed, and it skips the patcher's encounter-table scan entirely), and
    // only a genuine split sends `regular:...|boss:...`. A group asking for
    // nothing has to be spelled `1.0` rather than left empty, since an empty
    // segment is not a scale.
    //
    // Both groups at 1.0x collapses to '' - the identity - so "enabled but
    // asking for nothing" reads as retail rather than rewriting every monster
    // slot with its own values.
    let enemyStatScale = '';
    if (enemyScaleChk.checked) {
      const advanced = segVal('enemy_scale_mode', 'simple') === 'advanced';
      const bodyFor = (group) => {
        if (!advanced) {
          const f = enemyScaleSimple.get(group);
          return f ? Number(f.slider.value).toFixed(1) : '1.0';
        }
        return (enemyScaleFields.get(group) || [])
          .filter((f) => Number(f.slider.value) !== 1)
          .map((f) => `${f.key}=${Number(f.slider.value).toFixed(1)}`)
          .join(',');
      };
      const bodies = SCALE_GROUPS.map(([g]) => [g, bodyFor(g)]);
      const retail = (b) => b === '' || b === '1.0';
      if (bodies.every(([, b]) => retail(b))) enemyStatScale = '';
      else if (bodies[0][1] === bodies[1][1]) enemyStatScale = bodies[0][1];
      else enemyStatScale = bodies.map(([g, b]) => `${g}:${b || '1.0'}`).join('|');
    }
    // EXP multiplier: like the difficulty scale, "enabled but at 1.0x" is the
    // identity and collapses to '' (retail) rather than rewriting every slot.
    // Fixed to one decimal for the same float-step reason.
    const expScaleNum = expScaleChk.checked ? Number(expScaleSlider.value) : 1;
    const expScale = expScaleNum !== 1 ? expScaleNum.toFixed(1) : '';
    // Seru catch rate: every flat percent is a real override (100% is not the
    // identity - retail rates vary per monster), so send whatever is set.
    const seruCatchRate = seruCatchChk.checked ? String(seruCatchSlider.value) : '';
    const delilasParty = delilasPartySel.value;
    const delilasArtsVoice = delilasParty ? delilasArtsVoiceSel.value : '';
    const delilasMoves = delilasParty ? delilasMovesSel.value : '';
    // Enemy attack count: a multiplier like the EXP scale - "enabled at 1.0x"
    // is the identity and collapses to '' (retail).
    const attackCountNum = attackCountChk.checked ? Number(attackCountSlider.value) : 1;
    const attackCount = attackCountNum !== 1 ? attackCountNum.toFixed(1) : '';
    // Art overrides = the per-art rows serialized to `combo=value` pairs,
    // merged with anything typed into the raw (advanced) inputs.
    const artOv = artBuilder.collect();
    if (artOv.error) {
      setStatus(artOv.error, 'err');
      return;
    }
    const artsPower = [artOv.power, (artsPowerInput.value || '').trim()]
      .filter(Boolean).join(', ');
    // Super Art names contain spaces, so this list is comma-separated only -
    // it is never merged with the combo-keyed power list above. Picker rows
    // first, then anything typed into the raw (advanced) input.
    const superArtPower = [artOv.superPower, (superArtPowerInput.value || '').trim()]
      .filter(Boolean).join(', ');
    const artsApGrant = [artOv.grant, (artsApGrantInput.value || '').trim()]
      .filter(Boolean).join(', ');
    const artsApCost = artOv.cost;
    const chests = segVal('chests', 'none');
    const shops = segVal('shops', 'none');
    const casino = segVal('casino', 'none');
    const steals = segVal('steals', 'none');
    const arts = segVal('arts', 'none');
    const doors = segVal('doors', 'none');
    const doorCoupling = segVal('door_coupling', 'coupled');
    const houseDoors = houseDoorsChk.checked ? 'shuffle' : 'none';
    const equipmentDrops = equipmentDropsChk.checked;
    const startingItems = parseInt(startingItemsSel.value, 10) || 0;
    const startingLevel = parseInt(startingLevelSel.value, 10) || 0;
    // Door of Wind: the count (0 = off). The checkbox enables it; the number
    // input (default 10) sets how many, clamped to 1..99.
    const doorOfWind = doorOfWindChk.checked
      ? Math.min(99, Math.max(1, parseInt(doorOfWindCountInput.value, 10) || 10))
      : 0;
    // Incense: same shape as Door of Wind (0 = off; count clamped to 1..99).
    const incense = incenseChk.checked
      ? Math.min(99, Math.max(1, parseInt(incenseCountInput.value, 10) || 10))
      : 0;
    // Convenience accessories: checkbox = seed one (count 1), else 0.
    const speedChain = speedChainChk.checked ? 1 : 0;
    const chickenHeart = chickenHeartChk.checked ? 1 : 0;
    const goodLuckBell = goodLuckBellChk.checked ? 1 : 0;
    const allWarps = allWarpsChk.checked;
    const unusedEnemies = unusedEnemiesChk.checked;
    const unusedItems = unusedItemsChk.checked;
    const monsterStats = segVal('monster_stats', 'none');
    const movePower = segVal('move_power', 'none');
    const elementAffinity = segVal('element_affinity', 'none');
    const spellCost = segVal('spell_cost', 'none');
    const equipBonus = segVal('equip_bonus', 'none');
    const weaponSpecialty = weaponSpecialtyChk.checked;

    const langActive = langSel.value !== '';
    const texSpecs = texture.specs();
    // Whether any randomizer / language option is active (textures aside) -
    // when false the patch_rom pass is skipped entirely.
    const baseActive = !(
      !langActive &&
      drops === 'none' && !equipmentDrops && encounters === 'none' &&
      chests === 'none' && shops === 'none' && casino === 'none' &&
      steals === 'none' && arts === 'none' && doors === 'none' &&
      houseDoors === 'none' && startingItems === 0 && doorOfWind === 0 && incense === 0 &&
      speedChain === 0 && chickenHeart === 0 && goodLuckBell === 0 && !allWarps &&
      monsterStats === 'none' && movePower === 'none' && elementAffinity === 'none' &&
      spellCost === 'none' && equipBonus === 'none' && !weaponSpecialty &&
      startingLevel === 0 && !fleeExp && !seruTrade && !enemyAlly && !shinySeru && !showSuperArts && !jewelFix && !approachFix && !delilasChallenge && !customItems &&
      !fishingPrice && !renameLocation && !earthEggPrice && !artsPower && !superArtPower &&
      !artsApGrant && !artsApCost &&
      !spiritAp && !damageAp && !enemyStatScale && !expScale && !seruCatchRate && !delilasParty &&
      !attackCount && !swingCosts && !equipOwners
    );
    if (!baseActive && texSpecs.length === 0) {
      setStatus('Enable at least one option (pick a preset, a language, a texture, or flip a toggle).', 'err');
      return;
    }
    // The arena conflicts, described in terms of the controls that cause them
    // (see arenaConflictMessage). The banner already says this live; repeating
    // it verbatim here means the submit error is never a new sentence to parse.
    const arenaMsg = syncArenaConflict();
    if (arenaMsg) {
      setStatus(arenaMsg, 'err');
      return;
    }
    const seed = (seedInput.value || '').trim() || String(Date.now());

    runBtn.disabled = true;
    summaryEl.textContent = '';
    try {
      const mod = await ensureWasm(setStatus);
      setStatus('Reading disc image ...');
      const buf = new Uint8Array(await file.arrayBuffer());
      let langPack = '';
      if (langActive) {
        setStatus('Loading language pack ...');
        langPack = await resolveLangPack(langSel, langFile);
      }
      setStatus('Patching (this can take a moment for a full disc) ...');
      // Yield so the status paints before the disc buffer is copied into WASM
      // memory (that copy happens before the first progress stage fires).
      await new Promise((r) => setTimeout(r, 30));
      let data = buf;
      let usedSeed = null;
      let summaryText = '';
      let langReport = null;
      if (baseActive) {
        const result = await mod.patch_rom(buf, seed, langPack, drops, encounters, encounterScope, chests, shops, casino, steals, arts, doors, doorCoupling, houseDoors, startingItems, doorOfWind, incense, speedChain, chickenHeart, goodLuckBell, allWarps, unusedEnemies, unusedItems, equipmentDrops, monsterStats, movePower, elementAffinity, spellCost, equipBonus, weaponSpecialty, startingLevel, soloStrong, fleeExp, seruTrade, enemyAlly, shinySeru, jewelFix, approachFix, delilasChallenge, customItems, fishingPrice, renameLocation, earthEggPrice, artsPower, artsApGrant, artsApCost, spiritAp, damageAp, enemyStatScale, expScale, seruCatchRate, delilasParty, delilasArtsVoice, delilasMoves, superArtPower, showSuperArts, attackCount, swingCosts, equipOwners, onPatchProgress);
        data = result.data;
        usedSeed = result.seed;
        summaryText = result.summary || '';
        langReport = result.lang;
      }
      if (texSpecs.length) {
        setStatus('Applying texture replacement' + (texSpecs.length > 1 ? 's' : '') + ' ...');
        await new Promise((r) => setTimeout(r, 30));
        const texResult = await mod.apply_texture_replacements(data, texSpecs, onPatchProgress);
        data = texResult.data;
        summaryText += texResult.summary || '';
      }
      const name = patchedName(file.name, usedSeed || 'textures');
      triggerDownload(data, name);
      // Also emit a matching .cue (same base name) so the patched .bin loads in
      // emulators that expect a cue sheet. Sequenced after a tick because some
      // browsers throttle back-to-back programmatic downloads.
      const cueName = name.replace(/\.bin$/i, '.cue');
      const cueBytes = new TextEncoder().encode(cueFor(name));
      setTimeout(() => triggerDownload(cueBytes, cueName), 500);
      setStatus('Done. Downloaded ' + name + ' + ' + cueName, 'ok');
      summaryEl.textContent =
        (usedSeed ? 'seed: ' + usedSeed + '\n' : '') + summaryText +
        langCoverageText(langReport) +
        '\nLoad the .cue in your emulator (it points at the .bin); keep both files together.';
    } catch (e) {
      setStatus('Error: ' + (e && e.message ? e.message : e), 'err');
    } finally {
      hidePatchProgress();
      runBtn.disabled = false;
    }
  });

  // Live-resolve the seed string to its numeric value as a hint.
  seedInput.addEventListener('change', async () => {
    const s = (seedInput.value || '').trim();
    if (!s) return;
    try {
      const mod = await ensureWasm(setStatus);
      setStatus('seed "' + s + '" -> ' + mod.resolve_seed(s));
    } catch {
      /* ignore */
    }
  });
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', init);
} else {
  init();
}
