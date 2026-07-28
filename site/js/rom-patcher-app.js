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
 * jewel_fix, approach_softlock_fix, fishing_prices, location_renames,
 * earth_egg_price, arts_powers,
 * arts_ap_grants, arts_ap_costs, spirit_ap, damage_ap, enemy_stat_scale)
 * -> { data, summary, seed, lang }`, `resolve_seed(str)`,
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
 * -> { max_name_len, locations: [name x16], fishing: [{ page, row, item, name,
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
 * `apply_texture_replacements(image, specs) -> { data, summary }` applies the
 * queue (chained after patch_rom's output, or run alone). Change packs use
 * `export_texture_pack(specs, name, author, note) -> String` and
 * `import_texture_pack(image, json, acceptHashMismatch)`.
 * Imports resolve relative to THIS file (site/js/), so the package at
 * site/wasm/ is `../wasm/...`. Shipped language packs are static assets under
 * site/lang/<lang>.yaml, fetched on demand (nothing is bundled into the WASM).
 */

let wasmMod = null;

async function ensureWasm(setStatus) {
  if (wasmMod) return wasmMod;
  setStatus('Loading patcher (WASM) ...');
  wasmMod = await import('../wasm/legaia_web_viewer.js');
  await wasmMod.default();
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

  let maxNameLen = 31;

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

  function renderLocations(names) {
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
      maxNameLen = t.max_name_len || 31;
      renderFish(t.fishing);
      renderLocations(t.locations);
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
        locations.push(`${input.dataset.index}=${v}`);
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
function makeArtRow(onRemove) {
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
  }

  const apMode = document.createElement('select');
  apMode.className = 'art-ap';
  for (const [v, t] of [['keep', 'Keep original'], ['cost', 'Costs AP'], ['grant', 'Gives AP back']]) {
    const o = document.createElement('option');
    o.value = v;
    o.textContent = t;
    apMode.appendChild(o);
  }

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

  main.appendChild(mkField('Art', pick));
  main.appendChild(mkField('AP', apMode));
  main.appendChild(mkField('Amount', amtWrap));
  main.appendChild(mkField('Damage', dmg));
  main.appendChild(remove);

  const note = document.createElement('div');
  note.className = 'art-row-note';

  row.appendChild(main);
  row.appendChild(note);

  const refresh = () => {
    const sel = pick.value ? pick.value.split(':') : null;
    const art = sel ? ART_TABLE.find((a) => a.c === sel[0] && a.k === sel[1]) : null;
    const grant = apMode.value === 'grant';
    amtWrap.parentElement.hidden = apMode.value === 'keep';
    amtSign.textContent = grant ? '+' : '';
    amtUnit.textContent = 'AP each use';
    if (!art) {
      note.textContent = 'Pick an art to change what it does.';
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
    return { clear() {}, collect() { return { power: '', grant: '', cost: '', error: '' }; } };
  }

  const addRow = () => {
    const row = makeArtRow(() => {
      row.remove();
      onEdit();
    });
    container.appendChild(row);
    return row;
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
      const seenPower = new Set();
      const seenAp = new Set();
      const fail = (error) => ({ power: '', grant: '', cost: '', error });
      for (const row of container.querySelectorAll('.art-row')) {
        const { pick, apMode, amt, dmg } = row.artControls;
        if (!pick.value) continue;
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
  const where = t.entry < 0 ? `gap +${off}`
    : t.section >= 0 ? `entry ${t.entry} sec ${t.section} +${off}`
      : `entry ${t.entry} +${off}`;
  return `${t.tier} ${where}`;
}

// The string a filter query is matched against. This IS the search
// vocabulary, so anything a person might reasonably type has to be in here -
// which is why the tier id, the CDNAME block and the curated label are all
// folded in even though only some of them are displayed on the cell.
function texHaystack(t) {
  return `${t.tier} ${texDesc(t)} ${t.width}x${t.height} ${t.bpp}bpp ${t.label || ''} ` +
    `${t.block || ''} ${t.replaceable ? 'replaceable' : 'read-only'}`.toLowerCase();
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
    add('Palettes', t.cluts > 0
      ? `${t.cluts}` + (t.tier === 'battle-equip' && t.cluts > 1
        ? ' - shown and replaced through the first; the others recolour the same pixels'
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
  enemyAlly: false, shinySeru: false, jewelFix: false, approachFix: false, fishingPrice: '', renameLocation: '', earthEggPrice: '', artsPower: '', artsApGrant: '', spiritAp: '', damageAp: '', enemyStatScale: '',
  startingItems: 0, doorOfWind: false, incense: false,
  speedChain: false, chickenHeart: false, goodLuckBell: false,
  allWarps: false,
  unusedEnemies: false, unusedItems: false,
  monster_stats: 'none', move_power: 'none', element_affinity: 'none',
  spell_cost: 'none', equip_bonus: 'none', weaponSpecialty: false,
  startingLevel: 0,
};

// Both gameplay presets hand the player a generous, fast-travel-ready start:
// every convenience item + accessory, all warps unlocked, the whole starting
// party at level 10, and 5 random consumables on top.
const STARTING_BUNDLE = {
  startingItems: 5, startingLevel: 10, allWarps: true,
  doorOfWind: true, incense: true,
  speedChain: true, chickenHeart: true, goodLuckBell: true,
};

// Equipment drops are additive - a code hook grants one extra random gear piece
// on a low per-battle chance, on top of the normal drop - so every gameplay
// preset turns them on; only vanilla leaves them off.
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
    monster_stats: 'shuffle', equip_bonus: 'shuffle', equipmentDrops: true,
    seruTrade: true, enemyAlly: true, shinySeru: true, jewelFix: true, approachFix: true,
    ...STARTING_BUNDLE,
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
    ...STARTING_BUNDLE,
  },
};

function init() {
  const fileInput = $('rom-file');
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
  const jewelFixChk = $('rom-jewel-fix');
  const approachFixChk = $('rom-approach-fix');
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
  // Live read-out next to each AP slider. `input` fires while dragging;
  // `change` (which drives markCustom/syncDependents) only fires on release.
  for (const [slider, out] of [[spiritApSlider, spiritApVal], [damageApSlider, damageApVal]]) {
    if (slider && out) slider.addEventListener('input', () => { out.textContent = slider.value; });
  }
  // The difficulty scale is a multiplier, so it reads out as "2.5x". Its step
  // is 0.1, and float steps can land on 2.5000000000000004 - fix the display
  // (and everything derived from it) to one decimal.
  const fmtScale = (v) => Number(v).toFixed(1) + 'x';
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
  const formEl = document.querySelector('.rom-form');
  const presetBar = $('rom-presets');
  const customChip = $('rom-preset-custom');
  if (!fileInput || !runBtn) return; // not on this page

  const setStatus = (msg, kind) => {
    statusEl.textContent = msg;
    statusEl.className = 'rom-status' + (kind ? ' rom-status-' + kind : '');
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
    jewelFixChk.checked = cfg.jewelFix;
    approachFixChk.checked = cfg.approachFix;
    fishingPriceInput.value = cfg.fishingPrice || '';
    renameLocationInput.value = cfg.renameLocation || '';
    earthEggPriceInput.value = cfg.earthEggPrice || '';
    spiritApChk.checked = cfg.spiritAp !== '' && cfg.spiritAp != null;
    spiritApSlider.value = String(spiritApChk.checked ? cfg.spiritAp : 32);
    spiritApVal.textContent = spiritApSlider.value;
    damageApChk.checked = cfg.damageAp !== '' && cfg.damageAp != null;
    damageApSlider.value = String(damageApChk.checked ? cfg.damageAp : 100);
    damageApVal.textContent = damageApSlider.value;
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
    artsApGrantInput.value = cfg.artsApGrant || '';
    artBuilder.clear();
    manualTables.clear();
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

  // Grey out controls that have no effect given the current state.
  function syncDependents() {
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
    const jewelFix = jewelFixChk.checked;
    const approachFix = approachFixChk.checked;
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
    // Art overrides = the per-art rows serialized to `combo=value` pairs,
    // merged with anything typed into the raw (advanced) inputs.
    const artOv = artBuilder.collect();
    if (artOv.error) {
      setStatus(artOv.error, 'err');
      return;
    }
    const artsPower = [artOv.power, (artsPowerInput.value || '').trim()]
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
      startingLevel === 0 && !fleeExp && !seruTrade && !enemyAlly && !shinySeru && !jewelFix && !approachFix &&
      !fishingPrice && !renameLocation && !earthEggPrice && !artsPower && !artsApGrant && !artsApCost &&
      !spiritAp && !damageAp && !enemyStatScale
    );
    if (!baseActive && texSpecs.length === 0) {
      setStatus('Enable at least one option (pick a preset, a language, a texture, or flip a toggle).', 'err');
      return;
    }
    if (shinySeru && (artsApGrant || artsApCost)) {
      setStatus('Shiny Seru and per-art AP overrides cannot be combined (they use the same injected-code arena) - turn one of them off.', 'err');
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
      // Yield so the status paints before the synchronous WASM call.
      await new Promise((r) => setTimeout(r, 30));
      let data = buf;
      let usedSeed = null;
      let summaryText = '';
      let langReport = null;
      if (baseActive) {
        const result = mod.patch_rom(buf, seed, langPack, drops, encounters, encounterScope, chests, shops, casino, steals, arts, doors, doorCoupling, houseDoors, startingItems, doorOfWind, incense, speedChain, chickenHeart, goodLuckBell, allWarps, unusedEnemies, unusedItems, equipmentDrops, monsterStats, movePower, elementAffinity, spellCost, equipBonus, weaponSpecialty, startingLevel, soloStrong, fleeExp, seruTrade, enemyAlly, shinySeru, jewelFix, approachFix, fishingPrice, renameLocation, earthEggPrice, artsPower, artsApGrant, artsApCost, spiritAp, damageAp, enemyStatScale);
        data = result.data;
        usedSeed = result.seed;
        summaryText = result.summary || '';
        langReport = result.lang;
      }
      if (texSpecs.length) {
        setStatus('Applying texture replacement' + (texSpecs.length > 1 ? 's' : '') + ' ...');
        await new Promise((r) => setTimeout(r, 30));
        const texResult = mod.apply_texture_replacements(data, texSpecs);
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
