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
  const enemyScaleSlider = $('rom-enemy-scale');
  const enemyScaleVal = $('rom-enemy-scale-val');
  // Live read-out next to each AP slider. `input` fires while dragging;
  // `change` (which drives markCustom/syncDependents) only fires on release.
  for (const [slider, out] of [[spiritApSlider, spiritApVal], [damageApSlider, damageApVal]]) {
    if (slider && out) slider.addEventListener('input', () => { out.textContent = slider.value; });
  }
  // The difficulty scale is a multiplier, so it reads out as "2.5x". Its step
  // is 0.1, and float steps can land on 2.5000000000000004 - fix the display
  // (and everything derived from it) to one decimal.
  const fmtScale = (v) => Number(v).toFixed(1) + 'x';
  if (enemyScaleSlider && enemyScaleVal) {
    enemyScaleSlider.addEventListener('input', () => {
      enemyScaleVal.textContent = fmtScale(enemyScaleSlider.value);
    });
  }
  // Advanced mode: one slider per stat, keyed by the patcher's own stat names.
  // Dropped silently if the markup is absent, so an older page still works.
  const enemyScaleFields = SCALE_STATS.map(([key]) => ({
    key,
    slider: $(`rom-enemy-scale-${key}`),
    out: $(`rom-enemy-scale-${key}-val`),
  })).filter((f) => f.slider && f.out);
  for (const f of enemyScaleFields) {
    f.slider.addEventListener('input', () => {
      f.out.textContent = fmtScale(f.slider.value);
    });
  }
  // Set every per-stat slider at once, and keep its read-out in step.
  const setScaleFields = (valueFor) => {
    for (const f of enemyScaleFields) {
      f.slider.value = String(valueFor(f.key));
      f.out.textContent = fmtScale(f.slider.value);
    }
  };
  // Seven sliders need a way back to neutral that isn't "drag each one".
  const enemyScaleReset = $('rom-enemy-scale-reset');
  if (enemyScaleReset) {
    enemyScaleReset.addEventListener('click', () => {
      setScaleFields(() => 1);
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
    // Difficulty scale. A config value is either a bare multiplier or a
    // `stat=mult` list, so the string itself picks the view mode - a preset that
    // shapes individual stats opens in Advanced without needing a second key.
    enemyScaleChk.checked = cfg.enemyStatScale !== '' && cfg.enemyStatScale != null;
    const scaleText = enemyScaleChk.checked ? String(cfg.enemyStatScale) : '';
    const perStat = scaleText.includes('=');
    setSeg('enemy_scale_mode', perStat ? 'advanced' : 'simple');
    enemyScaleSlider.value = String(!scaleText || perStat ? 1 : scaleText);
    enemyScaleVal.textContent = fmtScale(enemyScaleSlider.value);
    // Whatever the list doesn't name goes back to 1.0x, so no stale slider
    // survives a preset switch.
    const named = new Map(
      (perStat ? scaleText.split(/[,;\s]+/) : [])
        .filter(Boolean)
        .map((tok) => tok.split('='))
        .filter((kv) => kv.length === 2)
        .map(([k, v]) => [k.trim().toLowerCase(), v.trim()]),
    );
    // `defense` is the CLI's alias for both defense halves, so expand it rather
    // than dropping it - the sliders are what the emitted string is rebuilt
    // from, and a key nothing reads would silently apply a different difficulty
    // than the config asked for. Only this one alias is honoured here: a config
    // value should otherwise use the canonical SCALE_STATS keys, which are the
    // only ones this page ever emits.
    const bothDefenses = named.get('defense') ?? named.get('def');
    if (bothDefenses !== undefined) {
      for (const half of ['defense_high', 'defense_low']) {
        if (!named.has(half)) named.set(half, bothDefenses);
      }
    }
    setScaleFields((key) => named.get(key) ?? 1);
    artsPowerInput.value = cfg.artsPower || '';
    artsApGrantInput.value = cfg.artsApGrant || '';
    artBuilder.clear();
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
    // Seed, disc-file and the language selection are orthogonal to the
    // randomization config, so editing them must not flip the preset to "Custom".
    if (e.target && ['rom-seed', 'rom-file', 'rom-lang', 'rom-lang-file',
      'rom-lang-pal-file', 'rom-lang-fold'].includes(e.target.id)) return;
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
    const fishingPrice = (fishingPriceInput.value || '').trim();
    const renameLocation = (renameLocationInput.value || '').trim();
    const earthEggPrice = (earthEggPriceInput.value || '').trim();
    // AP sliders: only sent when their override checkbox is on ('' = retail).
    // Both ranges include 0 and negatives, so read the value as-is.
    const spiritAp = spiritApChk.checked ? String(spiritApSlider.value) : '';
    const damageAp = damageApChk.checked ? String(damageApSlider.value) : '';
    // Difficulty scale. Simple sends one multiplier for every stat; Advanced
    // sends a `stat=mult` list naming only the stats that actually move, which
    // is the same spelling the CLI takes. Every value is fixed to one decimal,
    // because a 0.1 float step can land on 2.5000000000000004 and the parser
    // rounds to thousandths.
    //
    // Advanced with every slider at 1.0x collapses to '' - the identity - so
    // "enabled but asking for nothing" reads as retail rather than rewriting
    // every monster slot with its own values.
    let enemyStatScale = '';
    if (enemyScaleChk.checked) {
      enemyStatScale = segVal('enemy_scale_mode', 'simple') === 'advanced'
        ? enemyScaleFields
          .filter((f) => Number(f.slider.value) !== 1)
          .map((f) => `${f.key}=${Number(f.slider.value).toFixed(1)}`)
          .join(',')
        : Number(enemyScaleSlider.value).toFixed(1);
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
    if (
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
    ) {
      setStatus('Enable at least one option (pick a preset, a language, or flip a toggle).', 'err');
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
      const result = mod.patch_rom(buf, seed, langPack, drops, encounters, encounterScope, chests, shops, casino, steals, arts, doors, doorCoupling, houseDoors, startingItems, doorOfWind, incense, speedChain, chickenHeart, goodLuckBell, allWarps, unusedEnemies, unusedItems, equipmentDrops, monsterStats, movePower, elementAffinity, spellCost, equipBonus, weaponSpecialty, startingLevel, soloStrong, fleeExp, seruTrade, enemyAlly, shinySeru, jewelFix, approachFix, fishingPrice, renameLocation, earthEggPrice, artsPower, artsApGrant, artsApCost, spiritAp, damageAp, enemyStatScale);
      const data = result.data;
      const usedSeed = result.seed;
      const name = patchedName(file.name, usedSeed);
      triggerDownload(data, name);
      // Also emit a matching .cue (same base name) so the patched .bin loads in
      // emulators that expect a cue sheet. Sequenced after a tick because some
      // browsers throttle back-to-back programmatic downloads.
      const cueName = name.replace(/\.bin$/i, '.cue');
      const cueBytes = new TextEncoder().encode(cueFor(name));
      setTimeout(() => triggerDownload(cueBytes, cueName), 500);
      setStatus('Done. Downloaded ' + name + ' + ' + cueName, 'ok');
      summaryEl.textContent =
        'seed: ' + usedSeed + '\n' + (result.summary || '') +
        langCoverageText(result.lang) +
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
