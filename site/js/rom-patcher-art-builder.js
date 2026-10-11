/* rom-patcher-art-builder.js - the Tactical-Art override builder.
 *
 * The arts and Super Art tables the picker lists, the per-art override rows
 * (damage, AP grant, AP cost, chains), and the injected-code arena conflict
 * messages that name the picker rows causing a clash.
 *
 * Module script, loaded after rom-patcher-page-controls.js and ahead of
 * rom-patcher-app.js; publishes on window.LegaiaRomPatcher. */

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
  if (chk('rom-super-arts-pack') && chk('rom-super-arts-pack').checked) {
    out.push({
      key: 'superArtsPack',
      label: 'Super Arts Pack (by ZetaPhoenix)',
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

// The Delilas Challenge and the Delilas party swap both rewrite the same piece
// of battle AI code, and the patcher refuses the pair. '' when they are not
// both on.
function delilasConflictMessage() {
  const challenge = document.getElementById('rom-delilas-challenge');
  const party = document.getElementById('rom-delilas-party');
  if (!challenge || !party || !challenge.checked || !party.value) return '';
  return (
    '\u201cthe Delilas Challenge\u201d cannot be combined with the Delilas party swap. ' +
    'Both rewrite the same piece of the Delilas siblings\u2019 battle AI, and only one of ' +
    'them can have it. To fix, either set the Delilas party swap to Off, or turn off ' +
    '\u201cthe Delilas Challenge\u201d (the checkbox in Gameplay).'
  );
}

// The one sentence shown both live and at submit, or '' when there is no
// conflict. Only the pairs the patcher refuses outright are reported: shiny
// Seru vs the Delilas Challenge is resolved in the patcher's favour (the
// challenge wins, shiny is skipped with a note) and is not an error.
function arenaConflictMessage() {
  const claims = arenaClaims();
  const hard =
    claims.some((c) => c.key === 'superArtsPack') && claims.length > 1
      ? claims
      : claims.some((c) => c.key === 'showSuperArts') && claims.length > 1
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

window.LegaiaRomPatcher = Object.assign(window.LegaiaRomPatcher || {}, {
  setupArtBuilder, arenaClaims, arenaConflictMessage, delilasConflictMessage,
});
