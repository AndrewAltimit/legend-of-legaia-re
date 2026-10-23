/* In-browser ROM patcher: drive the Track-1 randomizer (compiled to WASM) on a
 * user-supplied disc image, entirely client-side, and download the patched
 * image. Nothing is uploaded; the disc bytes never leave the browser.
 *
 * The WASM module (legaia_web_viewer) exposes `patch_rom(image, seed, lang_pack,
 * lang_relayout, drops, encounters, encounter_scope, chests, shops, casino, steals, arts,
 * doors, door_coupling, house_doors, starting_items, door_of_wind, incense,
 * speed_chain, chicken_heart, good_luck_bell, all_warps,
 * unused_enemies, unused_items, equipment_drops, monster_stats, move_power,
 * element_affinity, spell_cost, equip_bonus, weapon_specialty, starting_level,
 * solo_strong_encounters, flee_exp, seru_trade, enemy_ally, shiny_seru,
 * jewel_fix, approach_softlock_fix, delilas_challenge, custom_items, fishing_prices, location_renames,
 * earth_egg_price, arts_powers, super_art_powers,
 * arts_ap_grants, arts_ap_costs, spirit_ap, damage_ap, oscillating_ap, enemy_stat_scale,
 * exp_scale, seru_catch_rate, delilas_party, delilas_arts_voice,
 * delilas_moves, enemy_attack_count, swing_costs, equip_owners, enemy_hp_bar,
 * fortress_encounter_fix, conkram_freeze_fix,
 * progress?)
 * -> Promise<{ data, summary, seed, lang }>`, `resolve_seed(str)`,
 * `validate_lang_pack(image, yaml, relayout?) -> { ok, language, applied, skipped, message, report }`,
 * `validate_lang_pack(image, yaml, relayout?)` dry-runs the whole-sector disc
 * relayout when `relayout` is true (patch_rom's `lang_relayout`, the argument
 * right after `lang_pack`, applies it), `export_lang_pack(image, language,
 * resume?) -> yaml_string` (resume = a pack whose filled lines seed the
 * export), `strip_lang_pack(yaml) -> { yaml, kept, total }` (the shareable,
 * source-free shape), and
 * `lift_official_pack(usa_image, other_image, fold_accents, language?) -> { yaml,
 * language, exe, build, summary, tables, ... }` (the other-disc transfer: the
 * user supplies their OWN second disc - an official PAL localization or a
 * fan-patched disc - it is read in this tab, and the
 * lifted YAML is fed back through the normal `lang_pack` path so it gets the
 * same two-phase ordering and the same coverage report). `lang` / `report`
 * carry the per-section language-patch coverage: `{ language, applied,
 * already_applied, skipped, untranslated, sections: [{name, total, filled,
 * applied, already_applied, skipped}], reasons: [{reason, count}],
 * relayout_entries, relayout_sectors, issues: [{key, reason, message}] }` (null
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
 * `preview_texture_replace(image, tier, entry, section, offset, png, quantize,
 * view?)` validates one swap and returns the original plus the as-encoded
 * preview. A multi-palette TIM adds `texture_palette_info(...)` (palette
 * count, per-region map when known), `decode_texture_view(..., view)` and
 * `export_texture_as(..., format, view)` (`image` / `composite` / `strip` /
 * `indexed`; `view` -1 = in-game palettes, k = palette k) - the upload side
 * recognises every one of those shapes on its own;
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

/* The rest of the patcher page, from the sibling modules the page loads ahead
 * of this one (see the note above init). They publish on one namespace rather
 * than through import statements, so every file is a <script> tag the page
 * generator stamps with its own content hash - an import specifier would
 * carry none. */
const {
  $, setupInfoTips, segVal, setSeg,
  SCALE_STATS, SCALE_GROUPS, splitScaleGroups, PRESETS,
  setupEquipmentEditor, setupManualTables,
  setupArtBuilder, arenaClaims, arenaConflictMessage, delilasConflictMessage,
  setupTextureReplacer,
} = window.LegaiaRomPatcher;

let wasmMod = null;

async function ensureWasm(setStatus) {
  if (wasmMod) return wasmMod;
  setStatus('Loading patcher (WASM) ...');
  wasmMod = await window.LegaiaWasm.load();
  return wasmMod;
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

// The pack lifted from the user's own other disc this session (YAML string),
// or null. Held in memory only - it carries that disc's script, so it is never
// persisted and only leaves the tab if the user downloads it.
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
      throw new Error('read the text from your other disc first (button above)');
    }
    return liftedPack;
  }
  return fetchShippedPack(v);
}

/* shared helper: site/js/site-util.js */
const triggerDownload = window.LegaiaUtil.downloadFile;

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
  if (lang.relayout_entries) {
    lines.push(`  disc relayout: ${lang.relayout_entries} scene(s) grew by ` +
      `${lang.relayout_sectors} sector(s); the patched image is larger than the original`);
  }
  if (lang.relocated_names || lang.grown_monster_names) {
    lines.push(`  longer names: ${lang.relocated_names || 0} moved to free table space, ` +
      `${lang.grown_monster_names || 0} monster record(s) grown`);
  }
  if ((lang.issues || []).length) {
    lines.push('  "Download skipped lines" lists every one by key, to find and shorten in your pack.');
  }
  return lines.join('\n') + '\n';
}

// The skipped lines of a language report as CSV (key, reason, message), so a
// translator can find each one in their own pack by its key.
function langIssuesCsv(lang) {
  const q = (v) => '"' + String(v == null ? '' : v).replace(/"/g, '""') + '"';
  const rows = ['key,reason,message'];
  for (const i of (lang.issues || [])) rows.push([q(i.key), q(i.reason), q(i.message)].join(','));
  return rows.join('\n') + '\n';
}

// The `language:` header of a pack's YAML, or '' - enough to name files after
// a pack the user brought, without a YAML parser on the page.
function packLanguage(yaml) {
  const m = /^language:\s*['"]?([^'"\n]+?)['"]?\s*$/m.exec(yaml || '');
  return m ? m[1].trim() : '';
}

// A .cue for the patched .bin. Legend of Legaia (USA) is a single-track
// MODE2/2352 disc, so the cue is fixed except for the FILE line, which must
// reference the patched .bin's name. Emulators (mednafen et al.) load the .cue
// and error if it points at a missing file, so we ship a matching one.
function cueFor(binName) {
  return `FILE "${binName}" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n`;
}

// --- Page wiring --------------------------------------------------------------
// The editors init() mounts live in their own modules:
// rom-patcher-table-editors.js, rom-patcher-art-builder.js and
// rom-patcher-texture-replacer.js. The presets init() applies are in
// rom-patcher-presets.js, and the DOM helpers every one of them shares are in
// rom-patcher-page-controls.js. The page loads all five ahead of this file,
// page-controls first.

function init() {
  const fileInput = $('rom-file');
  // Disc-identity panel (js/disc-info.js, a classic script loaded before this
  // module): identifies the picked image (serial, region, build, PROT layout)
  // from a few sliced sectors, without reading the whole file.
  if (window.DiscInfo) window.DiscInfo.attachInput(fileInput);
  // Share the picked disc with the rest of the site (js/rom-cache.js, a
  // classic script loaded before this module), so the translation workbench
  // and the viewers open it without a second pick. This page still reads its
  // own input; the cache is only written, never read, here.
  if (fileInput && window.RomCache) {
    fileInput.addEventListener('change', () => {
      const f = fileInput.files && fileInput.files[0];
      if (f) window.RomCache.put(f).catch(() => {});
    });
  }
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
  const enemyHpBarChk = $('rom-enemy-hp-bar');
  const shinySeruChk = $('rom-shiny-seru');
  const showSuperArtsChk = $('rom-show-super-arts');
  const superArtsPackChk = $('rom-super-arts-pack');
  const jewelFixChk = $('rom-jewel-fix');
  const approachFixChk = $('rom-approach-fix');
  const fortressFixChk = $('rom-fortress-fix');
  const conkramFixChk = $('rom-conkram-fix');
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
  delilasPartySel.addEventListener('change', () => syncArenaConflict());
  const fishingPriceInput = $('rom-fishing-price');
  const renameLocationInput = $('rom-rename-location');
  const earthEggPriceInput = $('rom-earth-egg-price');
  const spiritApChk = $('rom-spirit-ap-on');
  const spiritApSlider = $('rom-spirit-ap');
  const spiritApVal = $('rom-spirit-ap-val');
  const damageApChk = $('rom-damage-ap-on');
  const damageApSlider = $('rom-damage-ap');
  const damageApVal = $('rom-damage-ap-val');
  const oscillatingApChk = $('rom-oscillating-ap-on');
  const oscillatingApSlider = $('rom-oscillating-ap');
  const oscillatingApVal = $('rom-oscillating-ap-val');
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
  for (const [slider, out] of [[spiritApSlider, spiritApVal], [damageApSlider, damageApVal], [oscillatingApSlider, oscillatingApVal], [seruCatchSlider, seruCatchVal]]) {
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
  const langCodeInput = $('rom-lang-code');
  const langLiftBtn = $('rom-lang-lift');
  const langLiftSaveBtn = $('rom-lang-lift-save');
  const langValidateBtn = $('rom-lang-validate');
  const langExportBtn = $('rom-lang-export');
  const langRelayoutRow = $('rom-lang-relayout-row');
  const langRelayoutChk = $('rom-lang-relayout');
  const langIssuesBtn = $('rom-lang-issues');
  const langStripBtn = $('rom-lang-strip');
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
    const v = langSel.value;
    if (langFileRow) langFileRow.hidden = v !== '__custom';
    if (langOfficialRow) langOfficialRow.hidden = v !== '__official';
    if (langRelayoutRow) langRelayoutRow.hidden = v === '';
    // Sharing is for the translator's own pack: a shipped pack is already
    // shareable, and a lifted one is another disc's text, not theirs to share.
    if (langStripBtn) langStripBtn.hidden = v !== '__custom';
    // With a pack chosen, the export carries its lines next to the English.
    langExportBtn.textContent = v === ''
      ? 'Export a starter pack from my disc'
      : 'Export a working copy of this pack (English alongside)';
  }
  langSel.addEventListener('change', () => { syncLangRow(); setLangStatus(''); showLangIssues(null); });
  syncLangRow();

  // The last language report with skipped lines (from a check or a patch), for
  // the "Download skipped lines" button.
  let lastLangReport = null;
  function showLangIssues(report) {
    lastLangReport = report && (report.issues || []).length ? report : null;
    if (langIssuesBtn) langIssuesBtn.hidden = !lastLangReport;
  }
  if (langIssuesBtn) {
    langIssuesBtn.addEventListener('click', () => {
      if (!lastLangReport) return;
      const code = lastLangReport.language || 'xx';
      triggerDownload(new TextEncoder().encode(langIssuesCsv(lastLangReport)),
        `legaia_${code}.skipped-lines.csv`);
    });
  }

  // "Make a shareable pack": strip the English out of the user's own filled
  // pack, leaving only their lines keyed by disc position (the CLI's
  // `translate strip`). Needs no disc.
  if (langStripBtn) {
    langStripBtn.addEventListener('click', async () => {
      try {
        const yaml = await resolveLangPack(langSel, langFile);
        const mod = await ensureWasm(setStatus);
        const r = mod.strip_lang_pack(yaml);
        const code = packLanguage(r.yaml) || 'xx';
        triggerDownload(new TextEncoder().encode(r.yaml), `legaia_${code}.yaml`);
        setLangStatus(`Downloaded legaia_${code}.yaml: ${r.kept} translated line(s), ` +
          'no English text. This is the file to share; others load it with "Import my own pack".', 'ok');
      } catch (e) {
        setLangStatus('Error: ' + (e && e.message ? e.message : e), 'err');
      }
    });
  }

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
      const relayout = !!(langRelayoutChk && langRelayoutChk.checked);
      if (relayout) setLangStatus('Checking, with the relayout (this takes a moment) ...');
      await new Promise((res) => setTimeout(res, 30));
      const r = mod.validate_lang_pack(buf, yaml, relayout);
      setLangStatus(`${langSel.options[langSel.selectedIndex].text}: ${r.message}`, 'ok');
      // Per-section dry-run coverage in the summary panel (same shape as the
      // post-patch report).
      if (r.report) summaryEl.textContent = langCoverageText(r.report).trim();
      showLangIssues(r.report);
    } catch (e) {
      setLangStatus('Error: ' + (e && e.message ? e.message : e), 'err');
    }
  });

  // "Read the text from my other disc": the other-disc transfer. The user
  // supplies a SECOND disc they own (an official PAL SCES build, or a
  // fan-patched disc of any Latin build - a patched USA disc included); it
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
      setLangStatus('Choose your other disc image (.bin) first.', 'err');
      return;
    }
    langLiftBtn.disabled = true;
    try {
      setLangStatus('Reading both discs (nothing is uploaded) ...');
      const mod = await ensureWasm(setStatus);
      const usa = await discBytes();
      const pal = new Uint8Array(await palFile.arrayBuffer());
      setLangStatus('Reading the text (this takes a moment) ...');
      await new Promise((r) => setTimeout(r, 30));
      const code = langCodeInput ? langCodeInput.value.trim() : '';
      const r = mod.lift_official_pack(usa, pal, langFoldChk.checked, code || undefined);
      liftedPack = r.yaml;
      langLiftSaveBtn.hidden = false;
      langLiftSaveBtn.dataset.lang = r.language;
      setLangStatus(
        `${r.language.toUpperCase()} text read from ${r.exe} (${r.build}). ` +
        'Now press "Patch & download" below - the coverage report will say how much of it fits.',
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
    triggerDownload(new TextEncoder().encode(liftedPack), `legaia_${code}.lifted.yaml`);
    setLangStatus(`Downloaded legaia_${code}.lifted.yaml - it holds the game's script, so keep it to yourself.`, 'ok');
  });

  // Re-lifting is required when the other disc, the language code or the
  // accent choice changes.
  const invalidateLift = () => {
    liftedPack = null;
    langLiftSaveBtn.hidden = true;
    setLangStatus('');
  };
  langPalFile.addEventListener('change', invalidateLift);
  langFoldChk.addEventListener('change', invalidateLift);
  if (langCodeInput) langCodeInput.addEventListener('change', invalidateLift);

  // "Export a starter pack from my disc": dump a source-bearing working pack the
  // user can edit. With a pack chosen (shipped, imported or lifted) its filled
  // lines seed the export, so a translator continues from it with the English
  // beside every line (the CLI's `translate init --resume`).
  langExportBtn.addEventListener('click', async () => {
    try {
      setLangStatus('Exporting from your disc ...');
      const mod = await ensureWasm(setStatus);
      const buf = await discBytes();
      const v = langSel.value;
      // No pack to continue from yet (nothing imported / lifted): a blank one.
      const noPack = (v === '__custom' && !(langFile.files && langFile.files[0])) ||
        (v === '__official' && !liftedPack);
      const resume = noPack ? '' : await resolveLangPack(langSel, langFile);
      const code = (v && !v.startsWith('__')) ? v : (packLanguage(resume) || 'en');
      const yaml = mod.export_lang_pack(buf, code, resume || undefined);
      const bytes = new TextEncoder().encode(yaml);
      // A PAL or Japanese disc exports its own script (dialog only), stamped
      // with its own language: name the file after it.
      const fileCode = noPack ? (packLanguage(yaml) || code) : code;
      triggerDownload(bytes, `legaia_${fileCode}.working.yaml`);
      setLangStatus(`Downloaded legaia_${fileCode}.working.yaml - fill the translation: fields and import it above. ` +
        'It contains the game\'s own text, so keep it to yourself and share the "shareable pack" instead.', 'ok');
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
    enemyHpBarChk.checked = !!cfg.enemyHpBar;
    shinySeruChk.checked = cfg.shinySeru;
    showSuperArtsChk.checked = !!cfg.showSuperArts;
    superArtsPackChk.checked = !!cfg.superArtsPack;
    jewelFixChk.checked = cfg.jewelFix;
    approachFixChk.checked = cfg.approachFix;
    fortressFixChk.checked = !!cfg.fortressFix;
    conkramFixChk.checked = !!cfg.conkramFix;
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
    if (oscillatingApChk) {
      oscillatingApChk.checked = cfg.oscillatingAp !== '' && cfg.oscillatingAp != null;
      oscillatingApSlider.value = String(oscillatingApChk.checked ? cfg.oscillatingAp : 20);
      oscillatingApVal.textContent = oscillatingApSlider.value;
    }
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
    const delilasMsg = delilasConflictMessage();
    const msg = delilasMsg || arenaConflictMessage();
    box.textContent = msg;
    box.hidden = !msg;
    const claims = msg && !delilasMsg ? arenaClaims() : [];
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
      superArtsPack: 'rom-super-arts-pack',
      shinySeru: 'rom-shiny-seru',
      delilasChallenge: 'rom-delilas-challenge',
    };
    const lit = new Set(claims.map((c) => BOX[c.key]).filter(Boolean));
    if (delilasMsg) lit.add(BOX.delilasChallenge);
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
    const oscillatingRow = $('rom-oscillating-ap-row');
    if (oscillatingRow) oscillatingRow.classList.toggle('is-disabled', !(oscillatingApChk && oscillatingApChk.checked));
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
      'rom-lang-pal-file', 'rom-lang-fold', 'rom-lang-relayout'].includes(e.target.id)
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
    const enemyHpBar = enemyHpBarChk.checked;
    const shinySeru = shinySeruChk.checked;
    const showSuperArts = showSuperArtsChk.checked;
    const superArtsPack = superArtsPackChk.checked;
    const jewelFix = jewelFixChk.checked;
    const approachFix = approachFixChk.checked;
    const fortressFix = fortressFixChk.checked;
    const conkramFix = conkramFixChk.checked;
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
    // Oscillating AP costs: the grant-side damage percent, '' = off.
    const oscillatingAp = oscillatingApChk && oscillatingApChk.checked ? String(oscillatingApSlider.value) : '';
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
      startingLevel === 0 && !fleeExp && !seruTrade && !enemyAlly && !shinySeru && !showSuperArts && !superArtsPack && !jewelFix && !approachFix && !delilasChallenge && !customItems &&
      !fishingPrice && !renameLocation && !earthEggPrice && !artsPower && !superArtPower &&
      !artsApGrant && !artsApCost &&
      !spiritAp && !damageAp && !oscillatingAp && !enemyStatScale && !expScale && !seruCatchRate && !delilasParty &&
      !attackCount && !swingCosts && !equipOwners && !enemyHpBar && !fortressFix && !conkramFix
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
        const langRelayout = langActive && !!(langRelayoutChk && langRelayoutChk.checked);
        const result = await mod.patch_rom(buf, seed, langPack, langRelayout, drops, encounters, encounterScope, chests, shops, casino, steals, arts, doors, doorCoupling, houseDoors, startingItems, doorOfWind, incense, speedChain, chickenHeart, goodLuckBell, allWarps, unusedEnemies, unusedItems, equipmentDrops, monsterStats, movePower, elementAffinity, spellCost, equipBonus, weaponSpecialty, startingLevel, soloStrong, fleeExp, seruTrade, enemyAlly, shinySeru, jewelFix, approachFix, delilasChallenge, customItems, fishingPrice, renameLocation, earthEggPrice, artsPower, artsApGrant, artsApCost, spiritAp, damageAp, oscillatingAp, enemyStatScale, expScale, seruCatchRate, delilasParty, delilasArtsVoice, delilasMoves, superArtPower, showSuperArts, superArtsPack, attackCount, swingCosts, equipOwners, enemyHpBar, fortressFix, conkramFix, onPatchProgress);
        data = result.data;
        usedSeed = result.seed;
        summaryText = result.summary || '';
        langReport = result.lang;
        showLangIssues(langReport);
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
